//! Port of `reference/packages/core/src/tool-output-store.ts`.
//!
//! Bounds managed tool output: when the contextual text exceeds the configured
//! limits it is written to `Global.Path.data/tool-output` and replaced by a
//! head/tail preview with a marker.

use crate::model::{ToolContent, ToolOutput};

pub const MAX_LINES: usize = 2_000;
pub const MAX_BYTES: usize = 50 * 1024;
const RETENTION_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct BoundInput {
    pub session_id: String,
    pub tool_call_id: String,
    pub output: ToolOutput,
}

#[derive(Debug, Clone)]
pub struct BoundResult {
    pub output: ToolOutput,
    pub output_paths: Vec<String>,
}

fn take_prefix(input: &str, maximum_bytes: usize) -> String {
    let mut bytes = 0usize;
    let mut content = String::new();
    for ch in input.chars() {
        let size = ch.len_utf8();
        if bytes + size > maximum_bytes {
            break;
        }
        content.push(ch);
        bytes += size;
    }
    content
}

fn take_suffix(input: &str, maximum_bytes: usize) -> String {
    let mut bytes = 0usize;
    let mut content = Vec::new();
    for ch in input.chars().rev() {
        let size = ch.len_utf8();
        if bytes + size > maximum_bytes {
            break;
        }
        content.push(ch);
        bytes += size;
    }
    content.iter().rev().collect()
}

fn preview(text: &str, max_lines: usize, max_bytes: usize) -> (String, String) {
    let lines: Vec<&str> = text.split('\n').collect();
    let head_lines = max_lines.div_ceil(2);
    let tail_lines = max_lines / 2;
    let sampled = if lines.len() <= max_lines {
        text.to_string()
    } else {
        let head = lines[..head_lines.min(lines.len())].join("\n");
        let tail = if tail_lines > 0 {
            lines[lines.len().saturating_sub(tail_lines)..].join("\n")
        } else {
            String::new()
        };
        if tail.is_empty() {
            head
        } else {
            format!("{head}\n{tail}")
        }
    };
    if sampled.len() <= max_bytes {
        if lines.len() <= max_lines {
            return (sampled, String::new());
        }
        let head = lines[..head_lines.min(lines.len())].join("\n");
        let tail = if tail_lines > 0 {
            lines[lines.len().saturating_sub(tail_lines)..].join("\n")
        } else {
            String::new()
        };
        return (head, tail);
    }
    let head_bytes = max_bytes.div_ceil(2);
    let tail_bytes = max_bytes / 2;
    (
        take_prefix(&sampled, head_bytes),
        take_suffix(&sampled, tail_bytes),
    )
}

fn bounded_preview(text: &str, marker: &str, max_lines: usize, max_bytes: usize) -> String {
    let marker_only = take_prefix(marker, max_bytes)
        .split('\n')
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n");
    let marker_bytes = marker.len();
    if max_lines <= 4 || max_bytes <= marker_bytes + 4 {
        return marker_only;
    }
    let (head, tail) = preview(text, max_lines - 4, max_bytes - marker_bytes - 4);
    if tail.is_empty() {
        format!("{head}\n\n{marker}")
    } else {
        format!("{head}\n\n{marker}\n\n{tail}")
    }
}

fn line_count(text: &str) -> usize {
    let mut count = 1;
    for ch in text.chars() {
        if ch == '\n' {
            count += 1;
        }
    }
    count
}

/// Managed output directory (mirrors `Global.Path.data` + `tool-output`).
pub fn managed_directory() -> std::path::PathBuf {
    let data = directories::ProjectDirs::from("", "", "opencode")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("opencode"));
    data.join("tool-output")
}

/// `ToolOutputStore.limits` — config-free defaults.
pub fn limits() -> (usize, usize) {
    (MAX_LINES, MAX_BYTES)
}

/// `ToolOutputStore.bound` from `reference/packages/core/src/tool-output-store.ts`.
pub fn bound(input: &BoundInput) -> Result<BoundResult, String> {
    let (max_lines, max_bytes) = limits();
    let media: Vec<ToolContent> = input
        .output
        .content
        .iter()
        .filter(|item| matches!(item, ToolContent::File { .. }))
        .cloned()
        .collect();
    let text: Vec<ToolContent> = input
        .output
        .content
        .iter()
        .filter(|item| matches!(item, ToolContent::Text { .. }))
        .cloned()
        .collect();

    let contextual = if input.output.content.is_empty() {
        serde_json::to_string_pretty(&input.output.structured)
            .map_err(|error| format!("Failed to encode tool output: {error}"))?
    } else {
        text.iter()
            .map(|item| match item {
                ToolContent::Text { text } => text.as_str(),
                _ => "",
            })
            .collect::<Vec<_>>()
            .join("")
    };

    if line_count(&contextual) <= max_lines && contextual.len() <= max_bytes {
        return Ok(BoundResult {
            output: input.output.clone(),
            output_paths: Vec::new(),
        });
    }

    let output_path = write_managed(&contextual)?;
    let marker = format!("... output truncated; full content saved to {output_path} ...");
    let mut content = vec![ToolContent::Text {
        text: bounded_preview(&contextual, &marker, max_lines, max_bytes),
    }];
    content.extend(media);

    Ok(BoundResult {
        output: ToolOutput {
            structured: input.output.structured.clone(),
            content,
        },
        output_paths: vec![output_path],
    })
}

fn write_managed(content: &str) -> Result<String, String> {
    let directory = managed_directory();
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to write tool output: {error}"))?;
    let file = directory.join(format!(
        "tool_{}",
        crate::util::identifier::ascending("tool")
    ));
    std::fs::write(&file, content)
        .map_err(|error| format!("Failed to write tool output: {error}"))?;
    Ok(file.to_string_lossy().to_string())
}

/// `ToolOutputStore.cleanup` — removes managed files older than 7 days.
pub fn cleanup() {
    let directory = managed_directory();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return;
    };
    let cutoff = chrono::Utc::now().timestamp_millis() as u64 - RETENTION_MILLIS;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("tool_") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        if modified < cutoff {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Helper for the managed directory path used by core tools.
pub fn global_data_dir() -> std::path::PathBuf {
    directories::ProjectDirs::from("", "", "opencode")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("opencode"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as JsonValue;

    #[test]
    fn passes_small_output_through() {
        let output = ToolOutput::make(
            serde_json::json!({ "ok": true }),
            vec![ToolContent::Text {
                text: "small".to_string(),
            }],
        );
        let result = bound(&BoundInput {
            session_id: "ses".into(),
            tool_call_id: "call".into(),
            output,
        })
        .unwrap();
        assert!(result.output_paths.is_empty());
    }

    #[test]
    fn bounds_large_output_to_preview() {
        let big = "x".repeat(60 * 1024);
        let output = ToolOutput::make(JsonValue::Null, vec![ToolContent::Text { text: big }]);
        let result = bound(&BoundInput {
            session_id: "ses".into(),
            tool_call_id: "call".into(),
            output,
        })
        .unwrap();
        assert_eq!(result.output_paths.len(), 1);
        let text = match &result.output.content[0] {
            ToolContent::Text { text } => text,
            _ => unreachable!(),
        };
        assert!(text.contains("... output truncated; full content saved to"));
    }
}
