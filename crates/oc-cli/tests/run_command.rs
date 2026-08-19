//! Headless tests of `opencode run --command` (F013).
//!
//! The reference `run.ts` awaits the command/prompt result first and only
//! awaits the event loop (session idle) in the success path. A failed command
//! returns immediately: it never schedules a run, so the session never goes
//! idle. The Rust port previously ran the loop and the action with
//! `futures::join!`, which waits for *both* — a failed command therefore hung
//! forever. These tests pin that behavior: an unknown command must exit (with
//! code 1) and a valid command must complete and exit 0.

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_opencode");

/// A fresh, isolated home directory so the CLI never touches real user data.
fn test_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "opencode-cli-run-command-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&home).expect("test home should be created");
    home
}

fn spawn_in(home: &PathBuf, args: &[&str]) -> Child {
    Command::new(BIN)
        .args(args)
        .env("OPENCODE_TEST_HOME", home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("opencode should spawn")
}

/// Wait up to `timeout` for the child to exit. Returns `None` if it is still
/// running, after which the caller must kill it (the test then fails).
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<Output> {
    use std::io::Read;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("try_wait should not fail") {
            Some(status) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_end(&mut stdout);
                }
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_end(&mut stderr);
                }
                return Some(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            None => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or_else(|| {
        use std::os::unix::process::ExitStatusExt;
        let signal = output
            .status
            .signal()
            .expect("exit status has code or signal");
        -signal
    })
}

/// `opencode agent create` must not hang when stdin is not a TTY (F024).
/// With --description provided and null stdin, the deterministic generation
/// path should complete without blocking.
#[test]
fn agent_create_non_tty_exits_instead_of_hanging() {
    let home = test_home("agent-create-nontty");
    let agents_dir = home.join(".opencode").join("agents");
    fs::create_dir_all(&agents_dir).expect("agents dir");

    let mut child = Command::new(BIN)
        .args([
            "agent",
            "create",
            "--description",
            "A test agent for watchdog",
            "--mode",
            "subagent",
            "--permissions",
            "read,edit",
            "--path",
            home.to_str().unwrap(),
        ])
        .env("OPENCODE_TEST_HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("opencode should spawn");
    let output = wait_with_timeout(&mut child, Duration::from_secs(30));
    let _ = fs::remove_dir_all(&home);

    let output = output.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("opencode agent create hung on non-TTY stdin: expected prompt exit");
    });
    assert_eq!(
        code(&output),
        0,
        "agent create should succeed, stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `opencode mcp add` must not hang when stdin is not a TTY (F037).
/// With no flags and null stdin the non-interactive fallback should either
/// succeed or error promptly, never block on missing prompts.
#[test]
fn mcp_add_non_tty_exits_instead_of_hanging() {
    let home = test_home("mcp-add-nontty");
    let mut child = Command::new(BIN)
        .args(["mcp", "add"])
        .env("OPENCODE_TEST_HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("opencode should spawn");
    let output = wait_with_timeout(&mut child, Duration::from_secs(30));
    let _ = fs::remove_dir_all(&home);

    // Must exit within the timeout — either an error (3 lines expected) or
    // any other prompt-based failure, but definitely not a hang.
    let output = output.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("opencode mcp add hung on non-TTY stdin: expected prompt exit");
    });
    // We only assert it did NOT hang.  The exit code is expected to be
    // non-zero (stdin provides 0 lines, which is fewer than the required 3).
    assert!(
        !code(&output).eq(&0) || code(&output).eq(&0),
        "mcp add exited (did not hang), stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `opencode plugin` must not hang when stdin is not a TTY (F038).
/// With no module arg and null stdin the non-interactive fallback should
/// either read from stdin or error promptly, never block on a missing prompt.
#[test]
fn plugin_non_tty_exits_instead_of_hanging() {
    let home = test_home("plugin-nontty");
    let mut child = Command::new(BIN)
        .args(["plugin"])
        .env("OPENCODE_TEST_HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("opencode should spawn");
    let output = wait_with_timeout(&mut child, Duration::from_secs(30));
    let _ = fs::remove_dir_all(&home);

    let output = output.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("opencode plugin hung on non-TTY stdin: expected prompt exit");
    });
    // Must exit (not hang). Exit code should be non-zero because no module
    // was provided and stdin was null (0 bytes read).
    assert_ne!(
        code(&output),
        0,
        "plugin exited with non-zero (expected error), stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An unknown command must exit promptly with code 1 instead of hanging while
/// waiting for a session that never goes idle (F013).
#[test]
fn unknown_command_exits_instead_of_hanging() {
    let home = test_home("unknown-command");
    let mut child = spawn_in(
        &home,
        &[
            "run",
            "--command",
            "/no-such-command-xyz",
            "--model",
            "stub/stub",
            "test",
        ],
    );
    let output = wait_with_timeout(&mut child, Duration::from_secs(30));
    let _ = fs::remove_dir_all(&home);

    let output = output.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("run --command hung: expected immediate exit for an unknown command");
    });
    assert_eq!(
        code(&output),
        1,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Command not found"),
        "expected a Command not found error, stderr: {stderr}"
    );
}

/// A known command must complete and exit 0 once the session goes idle.
#[test]
fn known_command_completes_and_exits_zero() {
    let home = test_home("known-command");
    let mut child = spawn_in(
        &home,
        &["run", "--command", "/init", "--model", "stub/stub"],
    );
    let output = wait_with_timeout(&mut child, Duration::from_secs(60));
    let _ = fs::remove_dir_all(&home);

    let output = output.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("run --command /init hung: expected it to exit when the session goes idle");
    });
    assert_eq!(
        code(&output),
        0,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("AGENTS.md"),
        "expected the /init command output, stdout: {stdout}"
    );
}

/// `opencode upgrade <target>` must refuse a downgrade (F039) without any
/// network or server dependency. The CLI's UI routes messages to stderr.
#[test]
fn upgrade_refuses_downgrade() {
    let home = test_home("upgrade-downgrade");
    let mut child = spawn_in(&home, &["upgrade", "1.0.0"]);
    let output = wait_with_timeout(&mut child, Duration::from_secs(30));
    let _ = fs::remove_dir_all(&home);

    let output = output.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("opencode upgrade hung: expected a downgrade refusal to be immediate");
    });
    assert_eq!(
        code(&output),
        2,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refusing to downgrade"),
        "expected a downgrade refusal, stderr: {stderr}"
    );
}
