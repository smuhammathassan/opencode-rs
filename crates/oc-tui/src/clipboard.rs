//! Small platform clipboard adapter used by prompt paste and message copy.
//!
//! OpenCode delegates clipboard access to the host terminal environment. Keep
//! the adapter dependency-free and try the native command available on each
//! desktop/terminal combination instead of silently treating the keybinding
//! as a no-op.

use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy)]
struct ClipboardCommand {
    program: &'static str,
    args: &'static [&'static str],
}

#[cfg(target_os = "macos")]
const COPY_COMMANDS: &[ClipboardCommand] = &[ClipboardCommand {
    program: "pbcopy",
    args: &[],
}];

#[cfg(target_os = "windows")]
const COPY_COMMANDS: &[ClipboardCommand] = &[ClipboardCommand {
    program: "clip.exe",
    args: &[],
}];

#[cfg(all(unix, not(target_os = "macos")))]
const COPY_COMMANDS: &[ClipboardCommand] = &[
    ClipboardCommand {
        program: "wl-copy",
        args: &[],
    },
    ClipboardCommand {
        program: "xclip",
        args: &["-selection", "clipboard"],
    },
    ClipboardCommand {
        program: "xsel",
        args: &["--clipboard", "--input"],
    },
];

#[cfg(target_os = "macos")]
const PASTE_COMMANDS: &[ClipboardCommand] = &[ClipboardCommand {
    program: "pbpaste",
    args: &[],
}];

#[cfg(target_os = "windows")]
const PASTE_COMMANDS: &[ClipboardCommand] = &[ClipboardCommand {
    program: "powershell.exe",
    args: &["-NoProfile", "-Command", "Get-Clipboard"],
}];

#[cfg(all(unix, not(target_os = "macos")))]
const PASTE_COMMANDS: &[ClipboardCommand] = &[
    ClipboardCommand {
        program: "wl-paste",
        args: &["--no-newline"],
    },
    ClipboardCommand {
        program: "xclip",
        args: &["-selection", "clipboard", "-o"],
    },
    ClipboardCommand {
        program: "xsel",
        args: &["--clipboard", "--output"],
    },
];

/// Copy text using the first available host clipboard provider.
pub fn copy(text: &str) -> Result<(), String> {
    let mut errors = Vec::new();
    for command in COPY_COMMANDS {
        match run_copy(command, text) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(error),
        }
    }
    Err(format!(
        "no clipboard provider available{}",
        if errors.is_empty() {
            String::new()
        } else {
            format!(": {}", errors.join("; "))
        }
    ))
}

/// Read text using the first available host clipboard provider.
pub fn paste() -> Result<String, String> {
    let mut errors = Vec::new();
    for command in PASTE_COMMANDS {
        match run_paste(command) {
            Ok(value) => return Ok(value),
            Err(error) => errors.push(error),
        }
    }
    Err(format!(
        "no clipboard provider available{}",
        if errors.is_empty() {
            String::new()
        } else {
            format!(": {}", errors.join("; "))
        }
    ))
}

fn run_copy(command: &ClipboardCommand, text: &str) -> Result<(), String> {
    let mut child = Command::new(command.program)
        .args(command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", command.program))?;
    child
        .stdin
        .take()
        .ok_or_else(|| format!("{}: stdin unavailable", command.program))?
        .write_all(text.as_bytes())
        .map_err(|error| format!("{}: write failed: {error}", command.program))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("{}: wait failed: {error}", command.program))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("{} exited with {}", command.program, output.status))
        } else {
            Err(format!(
                "{} exited with {}: {stderr}",
                command.program, output.status
            ))
        }
    }
}

fn run_paste(command: &ClipboardCommand) -> Result<String, String> {
    let output = Command::new(command.program)
        .args(command.args)
        .output()
        .map_err(|error| format!("{}: {error}", command.program))?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|error| format!("{}: invalid UTF-8: {error}", command.program))
    } else {
        Err(format!(
            "{} exited with {}: {}",
            command.program,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{COPY_COMMANDS, PASTE_COMMANDS};

    #[test]
    fn exposes_at_least_one_copy_and_paste_provider() {
        assert!(!COPY_COMMANDS.is_empty());
        assert!(!PASTE_COMMANDS.is_empty());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_fallbacks_include_wayland_and_x11() {
        assert!(COPY_COMMANDS
            .iter()
            .any(|command| command.program == "wl-copy"));
        assert!(COPY_COMMANDS
            .iter()
            .any(|command| command.program == "xclip"));
        assert!(PASTE_COMMANDS
            .iter()
            .any(|command| command.program == "wl-paste"));
        assert!(PASTE_COMMANDS
            .iter()
            .any(|command| command.program == "xclip"));
    }
}

fn which_cmd(name: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(name);
            if p.is_file() {
                return true;
            }
        }
    }
    false
}

/// From reference/packages/tui/src/clipboard.ts (`copyCommand`)
pub fn copy_command(os: &str, is_wayland: bool) -> Option<Vec<String>> {
    copy_command_with_lookup(os, is_wayland, which_cmd)
}

/// Parametric copy command resolver matching reference `copyCommand(os, wayland, has)`.
pub fn copy_command_with_lookup(
    os: &str,
    is_wayland: bool,
    has: impl Fn(&str) -> bool,
) -> Option<Vec<String>> {
    if (os == "darwin" || os == "macos") && has("osascript") {
        return Some(vec!["osascript".to_string()]);
    }
    if os == "linux" && is_wayland && has("wl-copy") {
        return Some(vec!["wl-copy".to_string()]);
    }
    if os == "linux" && has("xclip") {
        return Some(vec![
            "xclip".to_string(),
            "-selection".to_string(),
            "clipboard".to_string(),
        ]);
    }
    if os == "linux" && has("xsel") {
        return Some(vec![
            "xsel".to_string(),
            "--clipboard".to_string(),
            "--input".to_string(),
        ]);
    }
    if (os == "win32" || os == "windows") && has("powershell.exe") {
        return Some(vec![
            "powershell.exe".to_string(),
            "-NonInteractive".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; Set-Clipboard -Value ([Console]::In.ReadToEnd())".to_string(),
        ]);
    }
    None
}
