//! Permission prompt rendering.
//! From reference/packages/tui/src/routes/session/permission.tsx

use ratatui::style::{Modifier, Style};

use crate::components::text::{wrap_plain, StyledLine};
use crate::theme::{selected_foreground, Theme};
use crate::types::PermissionRequest;
use crate::util::path_format::format_path;

/// Stage of the permission flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStage {
    Permission,
    Always,
    Reject,
}

#[derive(Debug, Clone)]
pub struct PermissionState {
    pub stage: PermissionStage,
    pub selected: usize,
    pub expanded: bool,
    pub reject_input: String,
}

impl Default for PermissionState {
    fn default() -> Self {
        PermissionState {
            stage: PermissionStage::Permission,
            selected: 0,
            expanded: false,
            reject_input: String::new(),
        }
    }
}

/// Options for the main permission prompt.
pub fn options() -> Vec<&'static str> {
    vec!["Allow once", "Allow always", "Reject"]
}

/// Compute the (icon, title, body) for a permission request.
/// From reference/packages/tui/src/routes/session/permission.tsx (`info`)
pub fn info(
    request: &PermissionRequest,
    cwd: &str,
    body_width: usize,
    theme: &Theme,
) -> (String, String, Vec<StyledLine>) {
    let permission = request.permission.as_str();
    let meta = &request.metadata;
    let input = &request.metadata;

    let get = |key: &str| {
        input
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    };
    let body = |lines: Vec<String>| {
        lines
            .into_iter()
            .map(|l| vec![(l, Style::default().fg(theme.text))])
            .collect()
    };

    match permission {
        "edit" => {
            let filepath = meta
                .get("filepath")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            (
                "→".to_string(),
                format!("Edit {}", format_path(filepath, cwd, "")),
                diff_body(request, body_width, theme),
            )
        }
        "read" => {
            let file_path = get("filePath");
            (
                "→".to_string(),
                format!("Read {}", format_path(file_path, cwd, "")),
                body(vec![format!("Path: {}", format_path(file_path, cwd, ""))]),
            )
        }
        "glob" => {
            let pattern = get("pattern");
            (
                "✱".to_string(),
                format!("Glob \"{pattern}\""),
                body(vec![format!("Pattern: {pattern}")]),
            )
        }
        "grep" => {
            let pattern = get("pattern");
            (
                "✱".to_string(),
                format!("Grep \"{pattern}\""),
                body(vec![format!("Pattern: {pattern}")]),
            )
        }
        "list" => {
            let dir = get("path");
            (
                "→".to_string(),
                format!("List {}", format_path(dir, cwd, "")),
                body(vec![format!("Path: {}", format_path(dir, cwd, ""))]),
            )
        }
        "bash" => {
            let command = get("command");
            (
                "#".to_string(),
                "Shell command".to_string(),
                body(vec![format!("$ {command}")]),
            )
        }
        "task" => {
            let type_ = get("subagent_type");
            let desc = get("description");
            (
                "#".to_string(),
                format!(
                    "{} Task",
                    crate::util::locale::titlecase(if type_.is_empty() {
                        "Unknown"
                    } else {
                        type_
                    })
                ),
                body(vec![format!("◉ {desc}")]),
            )
        }
        "webfetch" => {
            let url = get("url");
            (
                "%".to_string(),
                format!("WebFetch {url}"),
                body(vec![format!("URL: {url}")]),
            )
        }
        "websearch" => {
            let query = get("query");
            (
                "◈".to_string(),
                format!("Web Search \"{query}\""),
                body(vec![format!("Query: {query}")]),
            )
        }
        "external_directory" => {
            let parent = meta.get("parentDir").and_then(serde_json::Value::as_str);
            let filepath = meta.get("filepath").and_then(serde_json::Value::as_str);
            let pattern = request.patterns.first();
            let derived = pattern.map(|p| {
                if p.contains('*') {
                    dirname(p)
                } else {
                    p.clone()
                }
            });
            let raw = parent.or(filepath).or(derived.as_deref()).unwrap_or("");
            let patterns: Vec<String> = request.patterns.clone();
            (
                "←".to_string(),
                format!("Access external directory {}", format_path(raw, cwd, "")),
                body(patterns.iter().map(|p| format!("- {p}")).collect()),
            )
        }
        "doom_loop" => (
            "⟳".to_string(),
            "Continue after repeated failures".to_string(),
            body(vec![
                "This keeps the session running despite repeated failures.".to_string(),
            ]),
        ),
        _ => (
            "⚙".to_string(),
            format!("Call tool {permission}"),
            body(vec![format!("Tool: {permission}")]),
        ),
    }
}

fn dirname(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn diff_body(request: &PermissionRequest, width: usize, theme: &Theme) -> Vec<StyledLine> {
    let diff = request
        .metadata
        .get("diff")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if diff.is_empty() {
        return vec![vec![(
            "No diff provided".to_string(),
            Style::default().fg(theme.text_muted),
        )]];
    }
    let mut lines: Vec<StyledLine> = Vec::new();
    for line in diff.lines() {
        let color = if line.starts_with('+') {
            theme.diff_added
        } else if line.starts_with('-') {
            theme.diff_removed
        } else if line.starts_with("@@") {
            theme.diff_context
        } else {
            theme.text_muted
        };
        let wrapped = wrap_plain(line, width);
        for wl in wrapped {
            let mut spans: StyledLine = Vec::new();
            for (text, _) in wl {
                spans.push((text, Style::default().fg(color)));
            }
            lines.push(spans);
        }
    }
    lines
}

/// Render the permission prompt into lines for the given stage.
pub fn render(
    request: &PermissionRequest,
    state: &PermissionState,
    cwd: &str,
    width: usize,
    height: usize,
    theme: &Theme,
) -> Vec<StyledLine> {
    let mut lines: Vec<StyledLine> = Vec::new();
    match state.stage {
        PermissionStage::Always => {
            lines.push(header_line("Always allow", theme));
            let always = &request.always;
            if always.len() == 1 && always[0] == "*" {
                lines.push(vec![(
                    format!(
                        "  This will allow {} until OpenCode is restarted.",
                        request.permission
                    ),
                    Style::default().fg(theme.text_muted),
                )]);
            } else {
                lines.push(vec![(
                    "  This will allow the following patterns until OpenCode is restarted"
                        .to_string(),
                    Style::default().fg(theme.text_muted),
                )]);
                for pattern in always {
                    lines.push(vec![(
                        format!("  - {pattern}"),
                        Style::default().fg(theme.text),
                    )]);
                }
            }
            lines.extend(option_footer(&["Confirm", "Cancel"], state.selected, theme));
        }
        PermissionStage::Reject => {
            lines.push(header_line("Reject permission", theme));
            lines.push(vec![(
                "  Tell OpenCode what to do differently".to_string(),
                Style::default().fg(theme.text_muted),
            )]);
            lines.push(vec![(
                format!("  {}", state.reject_input),
                Style::default().fg(theme.text),
            )]);
            lines.push(vec![(
                "  enter confirm   esc cancel".to_string(),
                Style::default().fg(theme.text),
            )]);
        }
        PermissionStage::Permission => {
            let (icon, title, body) = info(request, cwd, width.saturating_sub(6), theme);
            let header = vec![
                ("  △ ".to_string(), Style::default().fg(theme.warning)),
                (
                    "Permission required".to_string(),
                    Style::default().fg(theme.text),
                ),
            ];
            lines.push(header);
            let mut title_line = vec![("    ".to_string(), Style::default())];
            title_line.push((format!("{icon} "), Style::default().fg(theme.text_muted)));
            title_line.push((title, Style::default().fg(theme.text)));
            lines.push(title_line);
            let max_body = height.saturating_sub(6);
            for body_line in body.into_iter().take(max_body) {
                let mut spans = vec![("      ".to_string(), Style::default())];
                spans.extend(body_line);
                lines.push(spans);
            }
            lines.extend(option_footer(&options(), state.selected, theme));
            // Hint line.
            lines.push(vec![(
                format!(
                    "  ⇆ select · enter confirm · ctrl+f {}",
                    if state.expanded {
                        "minimize"
                    } else {
                        "fullscreen"
                    }
                ),
                Style::default().fg(theme.text_muted),
            )]);
        }
    }
    let _ = (width, height);
    lines
}

fn header_line(title: &str, theme: &Theme) -> StyledLine {
    vec![
        ("  △ ".to_string(), Style::default().fg(theme.warning)),
        (title.to_string(), Style::default().fg(theme.text)),
    ]
}

fn option_footer(options: &[&str], selected: usize, theme: &Theme) -> Vec<StyledLine> {
    let mut spans: StyledLine = Vec::new();
    spans.push(("  ".to_string(), Style::default()));
    for (idx, option) in options.iter().enumerate() {
        let active = idx == selected;
        spans.push((
            format!(" {option} "),
            Style::default()
                .fg(if active {
                    selected_foreground(theme.warning)
                } else {
                    theme.text_muted
                })
                .bg(if active {
                    theme.warning
                } else {
                    theme.background_menu
                })
                .add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        spans.push(("  ".to_string(), Style::default()));
    }
    vec![spans]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash_request() -> PermissionRequest {
        serde_json::from_value(json!({
            "id": "per_1", "sessionID": "ses_1", "permission": "bash",
            "patterns": [], "metadata": { "command": "rm -rf /tmp/x" }, "always": []
        }))
        .unwrap()
    }

    #[test]
    fn bash_info() {
        let request = bash_request();
        let (icon, title, _body) = info(&request, "/proj", 60, &Theme::dark());
        assert_eq!(icon, "#");
        assert_eq!(title, "Shell command");
    }

    #[test]
    fn edit_info_has_diff_body() {
        let request: PermissionRequest = serde_json::from_value(json!({
            "id": "per_2", "sessionID": "ses_1", "permission": "edit",
            "patterns": [], "metadata": { "filepath": "/proj/src/a.ts", "diff": "+added\n-removed" }, "always": []
        }))
        .unwrap();
        let (icon, title, body) = info(&request, "/proj", 60, &Theme::dark());
        assert_eq!(icon, "→");
        assert_eq!(title, "Edit src/a.ts");
        assert!(!body.is_empty());
    }

    #[test]
    fn render_permission_stage() {
        let request = bash_request();
        let state = PermissionState::default();
        let lines = render(&request, &state, "/proj", 60, 20, &Theme::dark());
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Permission required"))));
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Allow once"))));
    }
}
