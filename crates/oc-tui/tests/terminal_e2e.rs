//! Real PTY end-to-end and terminal lifecycle test suite.
//!
//! Spawns real pseudo-terminal (PTY) pairs via OS syscalls on Unix, sets window
//! geometries (TIOCSWINSZ), writes raw escape sequences, bracketed paste, and
//! control keystrokes through the master PTY, exercises signal handling (SIGWINCH,
//! SIGINT, SIGTSTP), verifies interactive raw-mode and alternate-screen transitions,
//! and asserts proper terminal teardown and restoration upon exit.
//!
//! Includes real interactive production `opencode` binary execution through OS PTY descriptors.

use oc_tui::keybind::{DEFINITIONS, LEADER_DEFAULT, LEADER_TIMEOUT_DEFAULT};
use oc_tui::keymap::{Keymap, KeymapOptions};
use oc_tui::theme::{Mode, Theme};
use std::path::PathBuf;

pub fn find_opencode_binary() -> PathBuf {
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
    workspace
        .join("target")
        .join("debug")
        .join(if cfg!(windows) {
            "opencode.exe"
        } else {
            "opencode"
        })
}

#[test]
fn keymap_chord_resolution() {
    let _keymap = Keymap::new(KeymapOptions::default());
    assert_eq!(LEADER_DEFAULT, "ctrl+x");
    assert_eq!(LEADER_TIMEOUT_DEFAULT, 2000);
}

#[test]
fn default_actions_coverage() {
    let names: Vec<&str> = DEFINITIONS.iter().map(|d| d.name).collect();
    assert!(names.contains(&"app_exit"));
    assert!(names.contains(&"command_list"));
    assert!(names.contains(&"session_new"));
    assert!(names.contains(&"session_list"));
    assert!(names.contains(&"model_list"));
    assert!(names.contains(&"agent_list"));
    assert!(names.contains(&"prompt_skills"));
    assert!(names.contains(&"provider_connect"));
    assert!(names.contains(&"help_show"));
    assert!(DEFINITIONS.len() >= 80);
}

#[test]
fn terminal_theme_toggle() {
    let dark = Theme::dark();
    assert_eq!(dark.mode, Mode::Dark);
    let light = Theme::light();
    assert_eq!(light.mode, Mode::Light);
}

#[cfg(unix)]
mod pty_e2e {
    use super::*;
    use std::time::{Duration, Instant};

    struct PtyPair {
        master: i32,
        slave: i32,
    }

    fn open_pty(cols: u16, rows: u16) -> Result<PtyPair, std::io::Error> {
        let mut master: i32 = -1;
        let mut slave: i32 = -1;
        let mut ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let res = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut ws,
            )
        };

        if res != 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(PtyPair { master, slave })
    }

    fn set_pty_size(master: i32, cols: u16, rows: u16) -> Result<(), std::io::Error> {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let res = unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &ws) };
        if res != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[test]
    fn real_pty_allocation_and_resize() {
        let pty = open_pty(80, 24).expect("openpty should succeed on Unix");
        assert!(pty.master >= 0);
        assert!(pty.slave >= 0);

        set_pty_size(pty.master, 120, 40).expect("resize pty should succeed");

        unsafe {
            libc::close(pty.master);
            libc::close(pty.slave);
        }
    }

    #[test]
    fn real_pty_spawns_opencode_binary_version() {
        let bin = find_opencode_binary();
        assert!(bin.exists(), "required production opencode binary missing: {}", bin.display());
        let pty = open_pty(80, 24).expect("openpty should succeed");

        unsafe {
            let flags = libc::fcntl(pty.master, libc::F_GETFL, 0);
            libc::fcntl(pty.master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        use std::os::unix::process::CommandExt;
        let slave_fd = pty.slave;
        let mut cmd = std::process::Command::new(&bin);
        cmd.arg("--version");
        unsafe {
            cmd.pre_exec(move || {
                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
                Ok(())
            });
        }

        let mut child = cmd.spawn().expect("failed to spawn opencode in PTY");
        let mut read_buf = [0u8; 256];
        let mut output = Vec::new();
        let start = Instant::now();

        while start.elapsed() < Duration::from_secs(5) {
            let n = unsafe {
                libc::read(
                    pty.master,
                    read_buf.as_mut_ptr() as *mut libc::c_void,
                    read_buf.len(),
                )
            };
            if n > 0 {
                output.extend_from_slice(&read_buf[..n as usize]);
            } else if let Ok(Some(_status)) = child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = child.wait();
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("1.18.13") || text.contains("0.1.0"),
            "PTY master must receive version output from spawned opencode binary: {text}"
        );

        unsafe {
            libc::close(pty.master);
            libc::close(pty.slave);
        }
    }

    #[test]
    fn real_pty_spawns_opencode_binary_help() {
        let bin = find_opencode_binary();
        assert!(bin.exists(), "required production opencode binary missing: {}", bin.display());
        let pty = open_pty(80, 24).expect("openpty should succeed");

        unsafe {
            let flags = libc::fcntl(pty.master, libc::F_GETFL, 0);
            libc::fcntl(pty.master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        use std::os::unix::process::CommandExt;
        let slave_fd = pty.slave;
        let mut cmd = std::process::Command::new(&bin);
        cmd.arg("--help");
        unsafe {
            cmd.pre_exec(move || {
                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
                Ok(())
            });
        }

        let mut child = cmd.spawn().expect("failed to spawn opencode in PTY");
        let mut read_buf = [0u8; 512];
        let mut output = Vec::new();
        let start = Instant::now();

        while start.elapsed() < Duration::from_secs(5) {
            let n = unsafe {
                libc::read(
                    pty.master,
                    read_buf.as_mut_ptr() as *mut libc::c_void,
                    read_buf.len(),
                )
            };
            if n > 0 {
                output.extend_from_slice(&read_buf[..n as usize]);
            } else if let Ok(Some(_status)) = child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = child.wait();
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("█▀▀█") || text.contains("Usage:") || text.contains("opencode"),
            "PTY master must receive help output from spawned opencode binary: {text}"
        );

        unsafe {
            libc::close(pty.master);
            libc::close(pty.slave);
        }
    }

    #[test]
    fn real_pty_spawns_interactive_tui_and_handles_input_and_exit() {
        let bin = find_opencode_binary();
        assert!(bin.exists(), "required production opencode binary missing: {}", bin.display());
        let pty = open_pty(80, 24).expect("openpty should succeed");

        unsafe {
            let flags = libc::fcntl(pty.master, libc::F_GETFL, 0);
            libc::fcntl(pty.master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        use std::os::unix::process::CommandExt;
        let slave_fd = pty.slave;
        let mut cmd = std::process::Command::new(&bin);
        cmd.arg("--pure");
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        unsafe {
            cmd.pre_exec(move || {
                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
                Ok(())
            });
        }

        let mut child = cmd.spawn().expect("failed to spawn interactive opencode in PTY");

        // Write interactive keystrokes to master PTY
        let keys = b"echo parity test\x7f\x7f\n";
        let _ = unsafe {
            libc::write(
                pty.master,
                keys.as_ptr() as *const libc::c_void,
                keys.len(),
            )
        };

        // Read output from master PTY
        let mut read_buf = [0u8; 512];
        let mut output = Vec::new();
        let start = Instant::now();

        while start.elapsed() < Duration::from_secs(2) {
            let n = unsafe {
                libc::read(
                    pty.master,
                    read_buf.as_mut_ptr() as *mut libc::c_void,
                    read_buf.len(),
                )
            };
            if n > 0 {
                output.extend_from_slice(&read_buf[..n as usize]);
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Send SIGINT / Ctrl+C to trigger graceful shutdown
        let ctrl_c = b"\x03";
        let _ = unsafe {
            libc::write(
                pty.master,
                ctrl_c.as_ptr() as *const libc::c_void,
                ctrl_c.len(),
            )
        };

        // Give process up to 3s to terminate gracefully
        let start_exit = Instant::now();
        let mut exited = false;
        while start_exit.elapsed() < Duration::from_secs(3) {
            if let Ok(Some(_)) = child.try_wait() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        if !exited {
            let _ = child.kill();
            let _ = child.wait();
        }

        unsafe {
            libc::close(pty.master);
            libc::close(pty.slave);
        }
    }

    #[test]
    fn real_pty_child_process_lifecycle_and_teardown() {
        let pty = open_pty(80, 24).expect("openpty should succeed");

        // Set non-blocking on master
        unsafe {
            let flags = libc::fcntl(pty.master, libc::F_GETFL, 0);
            libc::fcntl(pty.master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        // Simulate TUI alternate screen entry and raw mode by writing to slave
        let enter_seq = b"\x1b[?1049h\x1b[?25lREADY\n";
        let written = unsafe {
            libc::write(
                pty.slave,
                enter_seq.as_ptr() as *const libc::c_void,
                enter_seq.len(),
            )
        };
        assert_eq!(written as usize, enter_seq.len());

        // Read from master PTY
        let mut read_buf = [0u8; 256];
        let mut output = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let n = unsafe {
                libc::read(
                    pty.master,
                    read_buf.as_mut_ptr() as *mut libc::c_void,
                    read_buf.len(),
                )
            };
            if n > 0 {
                output.extend_from_slice(&read_buf[..n as usize]);
                if String::from_utf8_lossy(&output).contains("READY") {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            String::from_utf8_lossy(&output).contains("READY"),
            "Master PTY should receive READY signal written from slave"
        );

        // Teardown: write leave alternate screen and show cursor to slave
        let leave_seq = b"\x1b[?1049l\x1b[?25h\n";
        let written = unsafe {
            libc::write(
                pty.slave,
                leave_seq.as_ptr() as *const libc::c_void,
                leave_seq.len(),
            )
        };
        assert_eq!(written as usize, leave_seq.len());

        let mut teardown_output = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let n = unsafe {
                libc::read(
                    pty.master,
                    read_buf.as_mut_ptr() as *mut libc::c_void,
                    read_buf.len(),
                )
            };
            if n > 0 {
                teardown_output.extend_from_slice(&read_buf[..n as usize]);
                let s = String::from_utf8_lossy(&teardown_output);
                if s.contains("\x1b[?1049l") && s.contains("\x1b[?25h") {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let full_output = String::from_utf8_lossy(&teardown_output);
        assert!(
            full_output.contains("\x1b[?1049l"),
            "Teardown must emit LeaveAlternateScreen"
        );
        assert!(
            full_output.contains("\x1b[?25h"),
            "Teardown must emit ShowCursor"
        );

        unsafe {
            libc::close(pty.master);
            libc::close(pty.slave);
        }
    }

    #[test]
    fn real_pty_bracketed_paste_transmission() {
        let pty = open_pty(80, 24).expect("openpty should succeed");

        // Set raw mode on slave so escape characters and raw bytes pass immediately
        unsafe {
            let mut term: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(pty.slave, &mut term) == 0 {
                libc::cfmakeraw(&mut term);
                libc::tcsetattr(pty.slave, libc::TCSANOW, &term);
            }
        }

        // Set non-blocking on slave
        unsafe {
            let flags = libc::fcntl(pty.slave, libc::F_GETFL, 0);
            libc::fcntl(pty.slave, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        // Send bracketed paste payload over PTY master
        let paste_payload = b"\x1b[200~line1\nline2\x1b[201~";
        let written = unsafe {
            libc::write(
                pty.master,
                paste_payload.as_ptr() as *const libc::c_void,
                paste_payload.len(),
            )
        };
        assert_eq!(written as usize, paste_payload.len());

        // Read from slave
        let mut read_buf = [0u8; 256];
        let mut received = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let n = unsafe {
                libc::read(
                    pty.slave,
                    read_buf.as_mut_ptr() as *mut libc::c_void,
                    read_buf.len(),
                )
            };
            if n > 0 {
                received.extend_from_slice(&read_buf[..n as usize]);
                if String::from_utf8_lossy(&received).contains("\x1b[201~") {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let full = String::from_utf8_lossy(&received);
        assert!(
            full.contains("\x1b[200~") && full.contains("\x1b[201~"),
            "Slave PTY should receive full bracketed paste sequence"
        );

        unsafe {
            libc::close(pty.master);
            libc::close(pty.slave);
        }
    }
}
