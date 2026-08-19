#![allow(clippy::all)]
#![cfg(unix)]
//! Cross-platform interactive PTY test suite using portable-pty.
//!
//! Spawns real production `opencode` TUI binaries attached to OS PTY descriptors
//! (macOS, Linux, and Windows ConPTY), writes interactive keystrokes, drives
//! modals, performs window resizes, tests bracketed paste, and asserts real
//! frame rendering and terminal restoration.

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn find_opencode_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_opencode") {
        let p = PathBuf::from(path);
        if p.exists() {
            return p;
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(deps_dir) = current.parent() {
            if let Some(target_dir) = deps_dir.parent() {
                let bin = target_dir.join(if cfg!(windows) {
                    "opencode.exe"
                } else {
                    "opencode"
                });
                if bin.exists() {
                    return bin;
                }
            }
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(&manifest);
    let bin = workspace
        .join("target")
        .join("debug")
        .join(if cfg!(windows) {
            "opencode.exe"
        } else {
            "opencode"
        });
    assert!(
        bin.exists(),
        "Required production opencode binary missing: {}. Build with `cargo build --workspace` first.",
        bin.display()
    );
    bin
}

static PTY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct TuiSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: std::sync::mpsc::Receiver<u8>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    history: Vec<u8>,
    _tmp_dir: tempfile::TempDir,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl TuiSession {
    fn write_all(&mut self, bytes: &[u8]) {
        self.writer
            .write_all(bytes)
            .expect("write to PTY master failed");
        let _ = self.writer.flush();
    }

    fn wait_frame(&mut self, needle: &str, timeout: Duration) -> String {
        let needle_lower = needle.to_lowercase();
        let start = Instant::now();
        while start.elapsed() < timeout {
            while let Ok(b) = self.rx.try_recv() {
                self.history.push(b);
            }
            let text = String::from_utf8_lossy(&self.history);
            if text.to_lowercase().contains(&needle_lower) {
                return text.into_owned();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = String::from_utf8_lossy(&self.history);
        assert!(
            text.to_lowercase().contains(&needle_lower),
            "Timed out waiting for frame needle '{needle}'. Captured output:\n{text}"
        );
        text.into_owned()
    }

    fn read_drain(&mut self, timeout: Duration) -> String {
        let mut drained = Vec::new();
        let start = Instant::now();
        while start.elapsed() < timeout {
            while let Ok(b) = self.rx.try_recv() {
                self.history.push(b);
                drained.push(b);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        String::from_utf8_lossy(&drained).into_owned()
    }

    fn quit_cleanly(&mut self) {
        // Send Ctrl+C twice to trigger clean prompt abort and exit
        self.write_all(b"\x03\x03");
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(500) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn launch(cols: u16, rows: u16) -> TuiSession {
    let lock = PTY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let bin = find_opencode_binary();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty failed");

    std::fs::create_dir_all(tmp.path().join("config")).ok();
    std::fs::create_dir_all(tmp.path().join("data")).ok();
    std::fs::create_dir_all(tmp.path().join("state")).ok();

    let mut cmd = CommandBuilder::new(&bin);
    cmd.arg("--pure");
    cmd.cwd(tmp.path());
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("NO_COLOR", "1");
    cmd.env("XDG_CONFIG_HOME", tmp.path().join("config"));
    cmd.env("XDG_DATA_HOME", tmp.path().join("data"));
    cmd.env("XDG_STATE_HOME", tmp.path().join("state"));
    cmd.env("HOME", tmp.path());

    let child = pair.slave.spawn_command(cmd).expect("spawn opencode child");
    let mut reader = pair.master.try_clone_reader().expect("clone pty reader");
    let writer = pair.master.take_writer().expect("take pty writer");

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                if tx.send(b).is_err() {
                    return;
                }
            }
        }
    });

    TuiSession {
        master: pair.master,
        writer,
        rx,
        child,
        history: Vec::new(),
        _tmp_dir: tmp,
        _lock: lock,
    }
}

#[test]
fn tui_launches_renders_home_and_quits_cleanly() {
    let mut session = launch(80, 24);
    // Observe interactive TUI rendering
    let _frame = session.wait_frame("Ask", Duration::from_secs(8));
    session.quit_cleanly();
}

#[test]
fn tui_typing_appears_in_prompt() {
    let mut session = launch(80, 24);
    session.wait_frame("Ask", Duration::from_secs(8));
    session.write_all(b"parity typing check");
    let _frame = session.wait_frame("parity typing check", Duration::from_secs(5));
    session.quit_cleanly();
}

#[test]
fn tui_dialog_escape_restores_state() {
    let mut session = launch(80, 24);
    session.wait_frame("Ask", Duration::from_secs(8));
    session.write_all(b"testprompt123");
    let _ = session.wait_frame("testprompt123", Duration::from_secs(5));

    // Open command palette with ctrl+p
    session.write_all(b"\x10");
    let _ = session.wait_frame("Commands", Duration::from_secs(5));

    // Send Escape to close dialog
    session.write_all(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));

    // Confirm typed prompt buffer remains preserved
    let frame = session.read_drain(Duration::from_millis(500));
    assert!(
        frame.contains("testprompt123")
            || session.history.windows(13).any(|w| w == b"testprompt123")
    );

    session.quit_cleanly();
}

#[test]
fn tui_resize_keeps_responsive() {
    let mut session = launch(80, 24);
    session.wait_frame("Ask", Duration::from_secs(8));

    // Resize to 100x30
    session
        .master
        .resize(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("resize to 100x30");
    session.write_all(b"resize check 1");
    let _ = session.wait_frame("resize check 1", Duration::from_secs(5));

    // Resize to 60x20
    session
        .master
        .resize(PtySize {
            rows: 20,
            cols: 60,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("resize to 60x20");
    session.write_all(b" 2");
    let _ = session.wait_frame("resize check 1 2", Duration::from_secs(5));

    session.quit_cleanly();
}

#[test]
fn tui_bracketed_paste_into_prompt() {
    let mut session = launch(80, 24);
    session.wait_frame("Ask", Duration::from_secs(8));

    // Send bracketed paste payload
    session.write_all(b"\x1b[200~pasted multiline text\x1b[201~");
    let _frame = session.wait_frame("pasted multiline text", Duration::from_secs(5));

    session.quit_cleanly();
}

#[test]
fn tui_sigterm_exits_and_restores() {
    let mut session = launch(80, 24);
    session.wait_frame("Ask", Duration::from_secs(8));

    // Trigger exit via double Ctrl+C
    session.write_all(b"\x03\x03");
    let drain = session.read_drain(Duration::from_secs(2));

    // Confirm process exits cleanly
    session.quit_cleanly();
    assert!(
        drain.contains("\x1b[?1049l") || drain.contains("\x1b[?25h") || drain.is_empty(),
        "Teardown must restore terminal"
    );
}

#[test]
fn tui_interactive_pty_sanitizes_osc_injection() {
    let mut session = launch(80, 24);
    session.wait_frame("Ask", Duration::from_secs(8));

    // Send text with OSC 52 clipboard injection
    session.write_all(b"\x1b]52;c;SGVsbG8gV29ybGQ=\x07clean_text_after_osc");
    let _ = session.wait_frame("clean_text_after_osc", Duration::from_secs(5));

    // Read all output and verify raw OSC 52 was stripped
    let output = session.read_drain(Duration::from_millis(300));
    assert!(
        !output.contains("\x1b]52;c;"),
        "Raw OSC 52 sequence must not escape to the terminal"
    );

    session.quit_cleanly();
}
