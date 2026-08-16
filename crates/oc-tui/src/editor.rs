//! External-editor integration for prompt editing and session export.

use std::io::Write;
use std::process::Command;

/// Open the configured editor with `initial` and return the resulting file.
///
/// TUI raw mode and the alternate screen are suspended while the editor runs,
/// then restored even when spawning or reading the editor fails.
pub fn edit(initial: &str) -> Result<String, String> {
    let command = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "vi".into()
            }
        });
    let mut command_parts = parse_command(&command)?;
    let program = command_parts.remove(0);
    let path = std::env::temp_dir().join(format!(
        "opencode-edit-{}-{}.md",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    std::fs::write(&path, initial).map_err(|error| format!("write editor file: {error}"))?;

    let raw = crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
    let mut result = Ok(());
    if raw {
        result = crossterm::terminal::disable_raw_mode()
            .map_err(|error| format!("disable terminal raw mode: {error}"));
    }
    if result.is_ok() {
        let mut stdout = std::io::stdout();
        result = crossterm::execute!(stdout, crossterm::terminal::LeaveAlternateScreen)
            .map_err(|error| format!("leave alternate screen: {error}"));
        let _ = stdout.flush();
    }
    if result.is_ok() {
        let status = Command::new(&program)
            .args(&command_parts)
            .arg(&path)
            .status()
            .map_err(|error| format!("launch editor {program}: {error}"));
        result = match status {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("editor exited with {status}")),
            Err(error) => Err(error),
        };
    }

    let restore_result = restore_terminal(raw);
    let content = if result.is_ok() {
        std::fs::read_to_string(&path).map_err(|error| format!("read editor file: {error}"))
    } else {
        Err(result.unwrap_err())
    };
    let _ = std::fs::remove_file(&path);
    restore_result.and(content)
}

fn restore_terminal(raw: bool) -> Result<(), String> {
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .map_err(|error| format!("enter alternate screen: {error}"))?;
    if raw {
        crossterm::terminal::enable_raw_mode()
            .map_err(|error| format!("enable terminal raw mode: {error}"))?;
    }
    stdout
        .flush()
        .map_err(|error| format!("flush terminal: {error}"))
}

/// Parse the common `$EDITOR` form without invoking a shell.
///
/// Quotes and backslash escapes are supported so values such as
/// `code --wait` and `emacsclient --alternate-editor ''` work without making
/// arbitrary shell evaluation part of a TUI keybinding.
fn parse_command(input: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if character == active {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if escaped {
        return Err("editor command ends with an escape".into());
    }
    if quote.is_some() {
        return Err("editor command has an unterminated quote".into());
    }
    if !current.is_empty() {
        parts.push(current);
    }
    if parts.is_empty() {
        return Err("editor command is empty".into());
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::parse_command;

    #[test]
    fn parses_editor_program_and_arguments() {
        assert_eq!(
            parse_command("code --wait").unwrap(),
            vec!["code", "--wait"]
        );
        assert_eq!(
            parse_command("emacsclient --alternate-editor ''").unwrap(),
            vec!["emacsclient", "--alternate-editor"]
        );
    }

    #[test]
    fn rejects_malformed_editor_commands() {
        assert!(parse_command("code \\").is_err());
        assert!(parse_command("'code").is_err());
        assert!(parse_command("   ").is_err());
    }
}
