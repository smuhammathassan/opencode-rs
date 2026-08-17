//! Port of `reference/packages/opencode/src/tool/truncate.ts` and
//! `reference/packages/opencode/src/tool/truncation-dir.ts`.

use std::path::PathBuf;

use crate::util::{evaluate, identifier, Rule};

pub const MAX_LINES: usize = 2000;
pub const MAX_BYTES: usize = 50 * 1024;
const RETENTION_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;

/// `TRUNCATION_DIR` from `reference/packages/opencode/src/tool/truncation-dir.ts:4`
/// (`Global.Path.data` + `tool-output`).
pub fn truncation_dir() -> PathBuf {
    writable_data_dir().join("tool-output")
}

fn writable_data_dir() -> PathBuf {
    let preferred = std::env::var_os("OPENCODE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            directories::ProjectDirs::from("", "", "opencode")
                .map(|dirs| dirs.data_dir().to_path_buf())
        })
        .unwrap_or_else(|| std::env::temp_dir().join("opencode"));
    if std::fs::create_dir_all(&preferred).is_ok() {
        preferred
    } else {
        let fallback = std::env::temp_dir().join("opencode");
        let _ = std::fs::create_dir_all(&fallback);
        fallback
    }
}

pub fn glob() -> String {
    format!("{}/*", truncation_dir().to_string_lossy())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Result {
    pub content: String,
    pub truncated: bool,
    pub output_path: Option<String>,
}

#[derive(Default)]
pub struct Options {
    pub max_lines: Option<usize>,
    pub max_bytes: Option<usize>,
    pub direction: Option<Direction>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Head,
    Tail,
}

pub fn has_task_tool(agent_permission: Option<&[Rule]>) -> bool {
    let Some(permission) = agent_permission else {
        return false;
    };
    evaluate("task", "*", &[permission]).action != "deny"
}

/// `Truncate.write` from `reference/packages/opencode/src/tool/truncate.ts:68`.
pub fn write(text: &str) -> std::io::Result<String> {
    let dir = truncation_dir();
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(identifier::ascending("tool"));
    let path = file.to_string_lossy().to_string();
    std::fs::write(&file, text)?;
    Ok(path)
}

/// `Truncate.cleanup` from `reference/packages/opencode/src/tool/truncate.ts:54`.
pub fn cleanup() {
    let cutoff = identifier::timestamp(&identifier::create(
        "tool",
        "ascending",
        Some(now_millis().saturating_sub(RETENTION_MILLIS)),
    ));
    let Ok(entries) = std::fs::read_dir(truncation_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("tool_") {
            continue;
        }
        if identifier::timestamp(&name) >= cutoff {
            continue;
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

/// `Truncate.limits` from `reference/packages/opencode/src/tool/truncate.ts:75`.
/// The config-driven path (`tool_output.max_lines` / `max_bytes`) is wired
/// through `set_config_limits`.
pub fn limits() -> (usize, usize) {
    let configured = config_limits();
    (
        configured.0.unwrap_or(MAX_LINES),
        configured.1.unwrap_or(MAX_BYTES),
    )
}

static CONFIG_LIMITS: std::sync::OnceLock<std::sync::Mutex<(Option<usize>, Option<usize>)>> =
    std::sync::OnceLock::new();

/// TODO(integration): replace with `Config.Service` resolution.
pub fn set_config_limits(max_lines: Option<usize>, max_bytes: Option<usize>) {
    let slot = CONFIG_LIMITS.get_or_init(|| std::sync::Mutex::new((None, None)));
    let mut guard = slot.lock().unwrap();
    *guard = (max_lines, max_bytes);
}

fn config_limits() -> (Option<usize>, Option<usize>) {
    let slot = CONFIG_LIMITS.get_or_init(|| std::sync::Mutex::new((None, None)));
    *slot.lock().unwrap()
}

/// `Truncate.output` from `reference/packages/opencode/src/tool/truncate.ts:85`.
pub fn output(text: &str, options: Options, agent_permission: Option<&[Rule]>) -> Result {
    let (default_max_lines, default_max_bytes) = limits();
    let max_lines = options.max_lines.unwrap_or(default_max_lines);
    let max_bytes = options.max_bytes.unwrap_or(default_max_bytes);
    let direction = options.direction.unwrap_or(Direction::Head);
    let lines: Vec<&str> = text.split('\n').collect();
    let total_bytes = text.len();

    if lines.len() <= max_lines && total_bytes <= max_bytes {
        return Result {
            content: text.to_string(),
            truncated: false,
            output_path: None,
        };
    }

    let mut out: Vec<&str> = Vec::new();
    let mut bytes = 0;
    let mut hit_bytes = false;

    match direction {
        Direction::Head => {
            let mut i = 0;
            while i < lines.len() && i < max_lines {
                let size = lines[i].len() + if i > 0 { 1 } else { 0 };
                if bytes + size > max_bytes {
                    hit_bytes = true;
                    break;
                }
                out.push(lines[i]);
                bytes += size;
                i += 1;
            }
        }
        Direction::Tail => {
            let mut i = lines.len();
            while i > 0 && out.len() < max_lines {
                i -= 1;
                let size = lines[i].len() + if !out.is_empty() { 1 } else { 0 };
                if bytes + size > max_bytes {
                    hit_bytes = true;
                    break;
                }
                out.insert(0, lines[i]);
                bytes += size;
            }
        }
    }

    let removed = if hit_bytes {
        total_bytes.saturating_sub(bytes)
    } else {
        lines.len().saturating_sub(out.len())
    };
    let unit = if hit_bytes { "bytes" } else { "lines" };
    let preview = out.join("\n");
    let file = match write(text) {
        Ok(file) => file,
        Err(error) => {
            return Result {
                content: format!("{preview}\n\n...{removed} {unit} truncated...\n\n(failed to save full output: {error})"),
                truncated: true,
                output_path: None,
            }
        }
    };

    let hint = if has_task_tool(agent_permission) {
        format!(
            "The tool call succeeded but the output was truncated. Full output saved to: {file}\nUse the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context."
        )
    } else {
        format!(
            "The tool call succeeded but the output was truncated. Full output saved to: {file}\nUse Grep to search the full content or Read with offset/limit to view specific sections."
        )
    };

    let content = match direction {
        Direction::Head => format!("{preview}\n\n...{removed} {unit} truncated...\n\n{hint}"),
        Direction::Tail => format!("...{removed} {unit} truncated...\n\n{hint}\n\n{preview}"),
    };

    Result {
        content,
        truncated: true,
        output_path: Some(file),
    }
}

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_small_output() {
        let result = output("hello", Options::default(), None);
        assert!(!result.truncated);
        assert_eq!(result.content, "hello");
    }

    #[test]
    fn truncates_head() {
        let text = (0..5000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = output(&text, Options::default(), None);
        assert!(result.truncated);
        assert!(result.output_path.is_some());
        assert!(result.content.starts_with("line 0\nline 1\n"));
        assert!(result.content.contains("truncated"));
    }

    #[test]
    fn truncates_tail() {
        let text = (0..5000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = output(
            &text,
            Options {
                direction: Some(Direction::Tail),
                ..Default::default()
            },
            None,
        );
        assert!(result.truncated);
        assert!(result.content.ends_with("line 4999"));
    }
}
