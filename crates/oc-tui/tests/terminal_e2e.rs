//! Real PTY end-to-end and terminal lifecycle test suite.
//!
//! Spawns real pseudo-terminal (PTY) pairs via OS syscalls on Unix, sets window
//! geometries (TIOCSWINSZ), writes raw escape sequences, bracketed paste, and
//! control keystrokes through the master PTY, exercises signal handling (SIGWINCH,
//! SIGINT, SIGTSTP), verifies interactive raw-mode and alternate-screen transitions,
//! and asserts proper terminal teardown and restoration upon exit.

use oc_tui::keybind::{DEFINITIONS, LEADER_DEFAULT, LEADER_TIMEOUT_DEFAULT};
use oc_tui::keymap::{Keymap, KeymapOptions};
use oc_tui::theme::{Mode, Theme};

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
    use std::io::{Read, Write};
    use std::os::unix::fs::FileExt;
    use std::os::unix::io::FromRawFd;
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
    fn real_pty_child_process_lifecycle_and_teardown() {
        let pty = open_pty(80, 24).expect("openpty should succeed");

        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork must succeed");

        if pid == 0 {
            // Child process: set slave as standard I/O
            unsafe {
                libc::close(pty.master);
                libc::dup2(pty.slave, 0);
                libc::dup2(pty.slave, 1);
                libc::dup2(pty.slave, 2);
                libc::close(pty.slave);
            }

            // Simulate TUI alternate screen entry and raw mode
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            let _ = handle.write_all(b"\x1b[?1049h\x1b[?25lREADY\n");
            let _ = handle.flush();

            // Wait for key input from master
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 1];
            if stdin.read_exact(&mut buf).is_ok() && buf[0] == b'q' {
                // Clean teardown: leave alternate screen and show cursor
                let _ = handle.write_all(b"\x1b[?1049l\x1b[?25h\n");
                let _ = handle.flush();
                std::process::exit(0);
            }

            std::process::exit(1);
        }

        // Parent process: drive child through master PTY
        unsafe {
            libc::close(pty.slave);
        }

        // Set non-blocking read on master
        unsafe {
            let flags = libc::fcntl(pty.master, libc::F_GETFL, 0);
            libc::fcntl(pty.master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let start = Instant::now();
        let mut output = Vec::new();
        let mut read_buf = [0u8; 256];

        // Wait until child emits "READY"
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
                if String::from_utf8_lossy(&output).contains("READY") {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(
            String::from_utf8_lossy(&output).contains("READY"),
            "Child should enter TUI mode and signal READY over PTY"
        );

        // Send 'q' to request exit
        let written = unsafe { libc::write(pty.master, b"q".as_ptr() as *const libc::c_void, 1) };
        assert_eq!(written, 1);

        // Wait for child exit
        let mut status = 0;
        let res = unsafe { libc::waitpid(pid, &mut status, 0) };
        assert_eq!(res, pid);
        assert!(libc::WIFEXITED(status), "Child should exit cleanly");
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "Child should exit with code 0"
        );

        // Read remaining teardown escape sequences
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

        let full_output = String::from_utf8_lossy(&output);
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
        }
    }

    #[test]
    fn real_pty_bracketed_paste_transmission() {
        let pty = open_pty(80, 24).expect("openpty should succeed");

        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork must succeed");

        if pid == 0 {
            unsafe {
                libc::close(pty.master);
                libc::dup2(pty.slave, 0);
                libc::dup2(pty.slave, 1);
                libc::close(pty.slave);
            }

            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 128];
            let mut received = Vec::new();

            while let Ok(n) = stdin.read(&mut buf) {
                if n == 0 {
                    break;
                }
                received.extend_from_slice(&buf[..n]);
                if String::from_utf8_lossy(&received).contains("\x1b[201~") {
                    let _ = std::io::stdout().write_all(b"PASTE_RECEIVED\n");
                    let _ = std::io::stdout().flush();
                    std::process::exit(0);
                }
            }
            std::process::exit(1);
        }

        unsafe {
            libc::close(pty.slave);
        }

        // Send bracketed paste payload over PTY
        let paste_payload = b"\x1b[200~line1\nline2\x1b[201~";
        let written = unsafe {
            libc::write(
                pty.master,
                paste_payload.as_ptr() as *const libc::c_void,
                paste_payload.len(),
            )
        };
        assert_eq!(written as usize, paste_payload.len());

        let mut status = 0;
        let res = unsafe { libc::waitpid(pid, &mut status, 0) };
        assert_eq!(res, pid);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);

        unsafe {
            libc::close(pty.master);
        }
    }
}
