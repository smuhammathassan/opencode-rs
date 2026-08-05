/// From reference/packages/opencode/src/util/process.ts
///
/// Child-process helpers mirroring `cross-spawn`-based `spawn`/`run`/`text`/
/// `lines`/`stop`. Stdio defaults to `ignore` for all three streams (as in the
/// reference) and `run` forces `stdout`/`stderr` to pipes.
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Child;

use crate::util::signal::Signal;

pub enum Env {
    /// `env: undefined` — inherit the parent environment.
    Inherit,
    /// `env: null` — an empty environment.
    Empty,
    /// `env: {...}` — the parent environment merged with the overrides.
    Override(HashMap<String, String>),
}

impl Default for Env {
    fn default() -> Self {
        Env::Inherit
    }
}

#[derive(Clone, Copy, Default)]
pub enum Stdio {
    #[default]
    Ignore,
    Pipe,
    Inherit,
}

pub enum Shell {
    Disabled,
    System,
    Program(String),
}

impl Default for Shell {
    fn default() -> Self {
        Shell::Disabled
    }
}

#[derive(Default)]
pub struct Options {
    pub cwd: Option<PathBuf>,
    pub env: Env,
    pub stdin: Stdio,
    pub stdout: Stdio,
    pub stderr: Stdio,
    pub shell: Shell,
    pub abort: Option<Arc<Signal>>,
    pub kill: Option<i32>,
    pub timeout: Option<u64>,
}

pub struct RunOptions {
    pub cwd: Option<PathBuf>,
    pub env: Env,
    pub stdin: Stdio,
    pub shell: Shell,
    pub abort: Option<Arc<Signal>>,
    pub kill: Option<i32>,
    pub timeout: Option<u64>,
    pub nothrow: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            cwd: None,
            env: Env::Inherit,
            stdin: Stdio::Ignore,
            shell: Shell::Disabled,
            abort: None,
            kill: None,
            timeout: None,
            nothrow: false,
        }
    }
}

#[derive(Debug)]
pub struct Result {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub struct TextResult {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub text: String,
}

/// `ProcessRunFailedError` — the `RunFailedError` thrown on non-zero exit.
#[derive(Debug)]
pub struct RunFailedError {
    pub cmd: Vec<String>,
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl fmt::Display for RunFailedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = String::from_utf8_lossy(&self.stderr).trim().to_string();
        if text.is_empty() {
            write!(
                f,
                "Command failed with code {}: {}",
                self.code,
                self.cmd.join(" ")
            )
        } else {
            write!(
                f,
                "Command failed with code {}: {}\n{}",
                self.code,
                self.cmd.join(" "),
                text
            )
        }
    }
}

impl std::error::Error for RunFailedError {}

#[derive(Debug)]
pub enum RunError {
    Spawn(std::io::Error),
    Failed(RunFailedError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::Spawn(e) => write!(f, "{e}"),
            RunError::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

fn to_stdio(stdio: Stdio) -> std::process::Stdio {
    match stdio {
        Stdio::Ignore => std::process::Stdio::null(),
        Stdio::Pipe => std::process::Stdio::piped(),
        Stdio::Inherit => std::process::Stdio::inherit(),
    }
}

fn shell_command(cmd: &[String], shell: &Shell) -> Vec<String> {
    let joined = cmd.join(" ");
    let program = match shell {
        Shell::Disabled => return cmd.to_vec(),
        Shell::System => {
            #[cfg(windows)]
            {
                "cmd.exe".to_string()
            }
            #[cfg(not(windows))]
            {
                "/bin/sh".to_string()
            }
        }
        Shell::Program(program) => program.clone(),
    };
    vec![program, "-c".to_string(), joined]
}

fn build_command(
    cmd: &[String],
    opts: &Options,
) -> std::result::Result<tokio::process::Command, std::io::Error> {
    if cmd.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Command is required",
        ));
    }
    if opts.abort.as_ref().is_some_and(|signal| signal.aborted()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "aborted",
        ));
    }

    let command = shell_command(cmd, &opts.shell);
    let mut child = tokio::process::Command::new(&command[0]);
    child.args(&command[1..]);
    if let Some(cwd) = &opts.cwd {
        child.current_dir(cwd);
    }
    match &opts.env {
        Env::Inherit => {}
        Env::Empty => {
            child.env_clear();
        }
        Env::Override(map) => {
            for (key, value) in map {
                child.env(key, value);
            }
        }
    }
    child.stdin(to_stdio(opts.stdin));
    child.stdout(to_stdio(opts.stdout));
    child.stderr(to_stdio(opts.stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    Ok(child)
}

#[cfg(unix)]
#[allow(unsafe_code)]
/// Sending an arbitrary signal to a pid requires the libc FFI call.
fn kill_pid(pid: u32, signal: i32) {
    let result = unsafe { libc::kill(pid as i32, signal) };
    if result == -1 {
        tracing::trace!(
            "kill({pid}, {signal}) failed: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(not(unix))]
fn kill_pid(pid: u32, _signal: i32) {
    let _ = pid;
}

fn attach_abort(pid: u32, opts: &Options) {
    let Some(signal) = opts.abort.clone() else {
        return;
    };
    let kill_signal = opts.kill.unwrap_or(libc_sigterm());
    let timeout_ms = opts.timeout.unwrap_or(5_000);
    tokio::spawn(async move {
        signal.wait().await;
        kill_pid(pid, kill_signal);
        if timeout_ms > 0 {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
            kill_pid(pid, libc_sigkill());
        }
    });
}

#[cfg(unix)]
fn libc_sigterm() -> i32 {
    libc::SIGTERM
}

#[cfg(not(unix))]
fn libc_sigterm() -> i32 {
    15
}

#[cfg(unix)]
fn libc_sigkill() -> i32 {
    libc::SIGKILL
}

#[cfg(not(unix))]
fn libc_sigkill() -> i32 {
    9
}

/// Spawns the command. The returned child has an attached abort handler when
/// `opts.abort` is set (SIGTERM, then SIGKILL after `opts.timeout` ms).
pub fn spawn(cmd: &[String], opts: &Options) -> std::result::Result<Child, std::io::Error> {
    let mut command = build_command(cmd, opts)?;
    let child = command.spawn()?;
    if let Some(pid) = child.id() {
        attach_abort(pid, opts);
    }
    Ok(child)
}

pub async fn wait(child: &mut Child) -> std::io::Result<i32> {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    let status = child.wait().await?;
    Ok(status
        .code()
        .unwrap_or(if status.signal().is_some() { 1 } else { 0 }))
}

pub async fn run(cmd: &[String], opts: &RunOptions) -> std::result::Result<Result, RunError> {
    let options = Options {
        cwd: opts.cwd.clone(),
        env: match &opts.env {
            Env::Inherit => Env::Inherit,
            Env::Empty => Env::Empty,
            Env::Override(map) => Env::Override(map.clone()),
        },
        stdin: opts.stdin,
        stdout: Stdio::Pipe,
        stderr: Stdio::Pipe,
        shell: match &opts.shell {
            Shell::Disabled => Shell::Disabled,
            Shell::System => Shell::System,
            Shell::Program(p) => Shell::Program(p.clone()),
        },
        abort: opts.abort.clone(),
        kill: opts.kill,
        timeout: opts.timeout,
    };
    let mut child = match spawn(cmd, &options) {
        Ok(child) => child,
        Err(e) => {
            if opts.nothrow {
                return Ok(Result {
                    code: 1,
                    stdout: Vec::new(),
                    stderr: crate::util::error::error_message(&crate::util::error::AnyError::Std(
                        &e,
                    ))
                    .into_bytes(),
                });
            }
            return Err(RunError::Spawn(e));
        }
    };

    let mut stdout = child.stdout.take().expect("stdout pipe");
    let mut stderr = child.stderr.take().expect("stderr pipe");
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let (code, out_read, err_read) = tokio::join!(
        wait(&mut child),
        tokio::io::AsyncReadExt::read_to_end(&mut stdout, &mut stdout_bytes),
        tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut stderr_bytes),
    );
    let code = match code {
        Ok(code) => code,
        Err(e) => {
            if opts.nothrow {
                return Ok(Result {
                    code: 1,
                    stdout: Vec::new(),
                    stderr: crate::util::error::error_message(&crate::util::error::AnyError::Std(
                        &e,
                    ))
                    .into_bytes(),
                });
            }
            return Err(RunError::Spawn(e));
        }
    };
    let _ = (out_read, err_read);

    let out = Result {
        code,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    };
    if out.code == 0 || opts.nothrow {
        return Ok(out);
    }
    Err(RunError::Failed(RunFailedError {
        cmd: cmd.to_vec(),
        code: out.code,
        stdout: out.stdout,
        stderr: out.stderr,
    }))
}

pub async fn text(cmd: &[String], opts: &RunOptions) -> std::result::Result<TextResult, RunError> {
    let out = run(cmd, opts).await?;
    Ok(TextResult {
        code: out.code,
        text: String::from_utf8_lossy(&out.stdout).into_owned(),
        stdout: out.stdout,
        stderr: out.stderr,
    })
}

pub async fn lines(
    cmd: &[String],
    opts: &RunOptions,
) -> std::result::Result<Vec<String>, RunError> {
    let out = text(cmd, opts).await?;
    Ok(out
        .text
        .split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

/// Mirrors `stop`: on non-Windows sends SIGTERM; on Windows uses `taskkill`
/// and falls back to a plain kill.
pub async fn stop(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let Some(pid) = child.id() else { return };
    #[cfg(windows)]
    {
        let out = run(
            &[
                "taskkill".to_string(),
                "/pid".to_string(),
                pid.to_string(),
                "/T".to_string(),
                "/F".to_string(),
            ],
            &RunOptions {
                nothrow: true,
                ..Default::default()
            },
        )
        .await;
        if out.map(|out| out.code).unwrap_or(1) == 0 {
            return;
        }
        kill_pid(pid, libc_sigterm());
    }
    #[cfg(not(windows))]
    {
        kill_pid(pid, libc_sigterm());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh_args(script: &str) -> Vec<String> {
        vec!["sh".to_string(), "-c".to_string(), script.to_string()]
    }

    #[tokio::test]
    async fn run_captures_output() {
        let out = run(&sh_args("echo hello"), &RunOptions::default())
            .await
            .unwrap();
        assert_eq!(out.code, 0);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
    }

    #[tokio::test]
    async fn text_and_lines() {
        let out = text(&sh_args("printf 'a\\nb\\n'"), &RunOptions::default())
            .await
            .unwrap();
        assert_eq!(out.text, "a\nb\n");
        assert_eq!(
            lines(&sh_args("printf 'a\\nb\\nc\\n'"), &RunOptions::default())
                .await
                .unwrap(),
            vec!["a", "b", "c"]
        );
    }

    #[tokio::test]
    async fn run_fails_on_non_zero() {
        let err = run(&sh_args("echo oops >&2; exit 3"), &RunOptions::default())
            .await
            .unwrap_err();
        match err {
            RunError::Failed(failed) => {
                assert_eq!(failed.code, 3);
                let message = failed.to_string();
                assert!(message.starts_with("Command failed with code 3:"));
                assert!(message.contains("oops"));
            }
            _ => panic!("expected RunFailedError"),
        }
    }

    #[tokio::test]
    async fn nothrow_returns_non_zero() {
        let out = run(
            &sh_args("exit 3"),
            &RunOptions {
                nothrow: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(out.code, 3);
    }

    #[tokio::test]
    async fn nothrow_masks_spawn_errors() {
        let out = run(
            &["definitely-not-a-command".to_string()],
            &RunOptions {
                nothrow: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(out.code, 1);
        assert!(!out.stderr.is_empty());
    }

    #[tokio::test]
    async fn spawn_errors_propagate() {
        let err = run(
            &["definitely-not-a-command".to_string()],
            &RunOptions::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RunError::Spawn(_)));
    }

    #[tokio::test]
    async fn empty_command_is_rejected() {
        let err = spawn(&[], &Options::default()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn env_override_and_cwd() {
        let dir = std::env::temp_dir();
        let out = run(
            &sh_args("pwd; echo \"$FOO\""),
            &RunOptions {
                cwd: Some(dir.clone()),
                env: Env::Override(HashMap::from([("FOO".to_string(), "bar".to_string())])),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(dir.to_string_lossy().as_ref()));
        assert!(text.contains("bar"));
    }

    #[tokio::test]
    async fn shell_system_runs_through_sh() {
        let out = run(
            &vec!["echo shell".to_string()],
            &RunOptions {
                shell: Shell::System,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "shell");
    }

    #[tokio::test]
    async fn abort_kills_process() {
        let signal = Signal::new();
        let mut options = Options::default();
        options.abort = Some(signal.clone());
        options.timeout = Some(500);
        let mut child = spawn(&sh_args("sleep 30"), &options).unwrap();
        signal.trigger();
        let code = wait(&mut child).await.unwrap();
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn stop_terminates_process() {
        let mut child = spawn(&sh_args("sleep 30"), &Options::default()).unwrap();
        stop(&mut child).await;
        let code = wait(&mut child).await.unwrap();
        assert_eq!(code, 1);
    }
}
