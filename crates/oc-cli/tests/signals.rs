//! Headless test of process-signal handling (F149): SIGTERM gracefully stops
//! the real `./opencode serve` process with a clean exit.
//!
//! Reference: `packages/opencode/src/index.ts` installs SIGINT/SIGTERM handlers
//! and the final `process.exit()` in `finally` returns the (0) status after a
//! clean handler completion; `util/process.ts` hooks the process-wide signal.
//! The Rust port forwards the first signal into a process-wide one-shot signal
//! that the `serve` command waits on before tearing the listener down.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

const BIN: &str = env!("CARGO_BIN_EXE_opencode");

#[cfg(unix)]
fn test_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "opencode-cli-signal-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&home).expect("test home should be created");
    home
}

/// Spawn `opencode serve --port 0` in an isolated home, wait for the "server
/// listening" line, send SIGTERM, and require a clean exit.
#[cfg(unix)]
#[test]
fn sigterm_gracefully_stops_serve_with_clean_exit() {
    let home = test_home();
    let mut child = Command::new(BIN)
        .args(["serve", "--port", "0"])
        .env("OPENCODE_TEST_HOME", &home)
        .env("OPENCODE_SERVER_PASSWORD", "test-password")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("opencode serve should spawn");

    let stdout = child.stdout.take().expect("child stdout");
    let reader = BufReader::new(stdout);
    let (line_tx, line_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in reader.lines() {
            match line {
                Ok(text) => {
                    if line_tx.send(text).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    let stderr = child.stderr.take().expect("child stderr");
    let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    thread::spawn({
        let stderr_buf = Arc::clone(&stderr_buf);
        move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                stderr_buf.lock().expect("stderr lock").push_str(&line);
            }
        }
    });

    // Wait up to 30s for the server to announce its listening address.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut saw_listening = false;
    while std::time::Instant::now() < deadline {
        match line_rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(line) => {
                if line.contains("server listening") {
                    saw_listening = true;
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(status) = child.try_wait().expect("try_wait") {
                    let _ = child.kill();
                    let detail = stderr_buf.lock().expect("stderr lock").clone();
                    panic!("serve exited ({status:?}) before listening: {detail}");
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let detail = stderr_buf.lock().expect("stderr lock").clone();
                panic!("serve stdout closed before listening: {detail}");
            }
        }
    }
    assert!(
        saw_listening,
        "serve never announced a listening address. stderr: {}",
        stderr_buf.lock().expect("stderr lock").clone()
    );

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => break status,
            None if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                panic!(
                    "serve did not exit after SIGTERM. stderr: {}",
                    stderr_buf.lock().expect("stderr lock").clone()
                );
            }
            None => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    };

    let _ = fs::remove_dir_all(&home);
    assert!(
        status.success(),
        "serve should exit cleanly after SIGTERM, got {status:?}. stderr: {}",
        stderr_buf.lock().expect("stderr lock").clone()
    );
}
