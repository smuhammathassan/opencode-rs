//! Subprocess runner.
//!
//! From reference/packages/core/src/process.ts — the `AppProcess.Service`
//! glue used by the git wrapper. Async on top of tokio.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout as timeout_at;

/// Error class mirroring `AppProcessError` (`_tag: "AppProcessError"`).
/// From reference/packages/core/src/process.ts
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppProcessError {
    pub _tag: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

impl AppProcessError {
    fn new(command: impl Into<String>) -> Self {
        AppProcessError {
            _tag: "AppProcessError".to_string(),
            command: command.into(),
            exit_code: None,
            stderr: None,
            cause: None,
        }
    }
}

impl std::fmt::Display for AppProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = self
            .stderr
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| self.cause.clone())
            .unwrap_or_default();
        let status = self
            .exit_code
            .map(|code| format!(" (exit {code})"))
            .unwrap_or_default();
        if detail.is_empty() {
            write!(f, "Command failed{status}: {}", self.command)
        } else {
            write!(f, "Command failed{status}: {}: {detail}", self.command)
        }
    }
}

impl std::error::Error for AppProcessError {}

#[derive(Debug, Clone)]
pub struct Command {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub extend_env: bool,
    pub stdin: Stdin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdin {
    Ignore,
    Pipe,
}

impl Command {
    pub fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Command {
            command: command.into(),
            args,
            cwd: None,
            env: None,
            extend_env: true,
            stdin: Stdin::Ignore,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub stdin: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
    pub max_output_bytes: Option<usize>,
    pub max_error_bytes: Option<usize>,
    pub combine_output: bool,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub command: String,
    pub exit_code: i32,
    pub output: Option<Vec<u8>>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub output_truncated: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

fn describe_command(command: &Command) -> String {
    if command.args.is_empty() {
        command.command.clone()
    } else {
        format!("{} {}", command.command, command.args.join(" "))
    }
}

/// Mirrors `AppProcess.run(command, options?)`.
pub async fn run(command: &Command, options: &RunOptions) -> Result<RunResult, AppProcessError> {
    let description = describe_command(command);
    let mut cmd = TokioCommand::new(&command.command);
    cmd.args(&command.args);
    if let Some(cwd) = &command.cwd {
        cmd.current_dir(cwd);
    }
    if let Some(env) = &command.env {
        cmd.envs(env);
    }
    if command.extend_env {
        cmd.env_clear();
        cmd.envs(std::env::vars());
        if let Some(env) = &command.env {
            cmd.envs(env);
        }
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(
        if options.stdin.is_some() || command.stdin == Stdin::Pipe {
            Stdio::piped()
        } else {
            Stdio::null()
        },
    );

    let run_fut = async {
        let mut child = cmd.spawn().map_err(|e| {
            let mut err = AppProcessError::new(description.clone());
            err.cause = Some(e.to_string());
            err
        })?;

        if let Some(input) = &options.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input).await.map_err(|e| {
                    let mut err = AppProcessError::new(description.clone());
                    err.cause = Some(format!("stdin: {e}"));
                    err
                })?;
            }
            // close stdin explicitly so the child observes EOF
            drop(child.stdin.take());
        }

        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let mut stdout_truncated = false;
        let mut stderr_truncated = false;

        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");
        let mut out_chunk = [0u8; 8192];
        let mut err_chunk = [0u8; 8192];

        // Read stdout and stderr concurrently.
        let (out_read, err_read) = tokio::join!(
            async {
                loop {
                    match stdout.read(&mut out_chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            append_limited(
                                &mut stdout_buf,
                                &out_chunk[..n],
                                options.max_output_bytes,
                                &mut stdout_truncated,
                            );
                        }
                    }
                }
            },
            async {
                loop {
                    match stderr.read(&mut err_chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            append_limited(
                                &mut stderr_buf,
                                &err_chunk[..n],
                                options.max_error_bytes,
                                &mut stderr_truncated,
                            );
                        }
                    }
                }
            }
        );
        let _ = (out_read, err_read);

        let status = child.wait().await.map_err(|e| {
            let mut err = AppProcessError::new(description.clone());
            err.cause = Some(e.to_string());
            err
        })?;
        let exit_code = status.code().unwrap_or(-1);

        if options.combine_output {
            let mut output = stdout_buf;
            output.extend_from_slice(&stderr_buf);
            Ok(RunResult {
                command: description.clone(),
                exit_code,
                output: Some(output),
                stdout: Vec::new(),
                stderr: Vec::new(),
                output_truncated: stdout_truncated || stderr_truncated,
                stdout_truncated: false,
                stderr_truncated: false,
            })
        } else {
            Ok(RunResult {
                command: description.clone(),
                exit_code,
                output: None,
                stdout: stdout_buf,
                stderr: stderr_buf,
                output_truncated: false,
                stdout_truncated,
                stderr_truncated,
            })
        }
    };

    match options.timeout {
        Some(duration) => match timeout_at(duration, run_fut).await {
            Ok(result) => result,
            Err(_) => {
                let mut err = AppProcessError::new(description.clone());
                err.cause = Some("Timed out".to_string());
                Err(err)
            }
        },
        None => run_fut.await,
    }
}

fn append_limited(buf: &mut Vec<u8>, chunk: &[u8], max: Option<usize>, truncated: &mut bool) {
    if let Some(max) = max {
        let remaining = max.saturating_sub(buf.len());
        let take = chunk.len().min(remaining);
        buf.extend_from_slice(&chunk[..take]);
        if chunk.len() > take {
            *truncated = true;
        }
    } else {
        buf.extend_from_slice(chunk);
    }
}

/// Mirrors `requireSuccess(result)`.
pub fn require_success(result: &RunResult) -> Result<(), AppProcessError> {
    if result.exit_code == 0 {
        Ok(())
    } else {
        Err(AppProcessError {
            _tag: "AppProcessError".to_string(),
            command: result.command.clone(),
            exit_code: Some(result.exit_code),
            stderr: Some(String::from_utf8_lossy(&result.stderr).to_string()),
            cause: None,
        })
    }
}
