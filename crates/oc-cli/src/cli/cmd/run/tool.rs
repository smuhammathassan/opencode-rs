//! Per-tool display rules for `opencode run` output.
//! From reference/packages/opencode/src/cli/cmd/run/tool.ts (the `run*`
//! helpers and `toolInlineInfo`).

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::types::Part;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolInlineMode {
    Inline,
    Block,
}

#[derive(Debug, Clone)]
pub struct ToolInline {
    pub icon: String,
    pub title: String,
    pub description: Option<String>,
    pub mode: ToolInlineMode,
    pub body: Option<String>,
}

fn list(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `Locale.titlecase` from reference/packages/tui/src/util/locale.ts.
pub fn titlecase(input: &str) -> String {
    let mut out = String::new();
    let mut capitalize = true;
    for ch in input.chars() {
        if ch.is_whitespace() {
            capitalize = true;
            out.push(ch);
        } else if capitalize && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            out.push(ch);
            capitalize = false;
        }
    }
    out
}

/// `webSearchProviderLabel` from reference/packages/opencode/src/tool/websearch.ts.
pub fn web_search_provider_label(provider: Option<&Value>) -> String {
    match provider.and_then(Value::as_str) {
        Some("parallel") => "Parallel Web Search".to_string(),
        Some("exa") => "Exa Web Search".to_string(),
        _ => "Web Search".to_string(),
    }
}

/// `toolPath` from tool.ts: relativize against cwd, then home.
pub fn tool_path(input: Option<&str>, opts_home: bool) -> String {
    let Some(input) = input else {
        return String::new();
    };
    if input.is_empty() {
        return String::new();
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let abs = if Path::new(input).is_absolute() {
        PathBuf::from(input)
    } else {
        cwd.join(input)
    };
    let rel = match abs.strip_prefix(&cwd) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_path_buf(),
        _ => return normalize(&abs),
    };
    if !rel.as_os_str().to_string_lossy().starts_with("..") {
        return normalize(&rel);
    }
    if opts_home && !home.as_os_str().is_empty() && (abs == home || abs.starts_with(&home)) {
        let rest = abs.strip_prefix(&home).unwrap_or(&abs);
        return format!("~{}", normalize(rest));
    }
    normalize(&abs)
}

fn normalize(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// `info()` from tool.ts: `[k=v, ...]` for string/number/bool fields.
fn info(data: &serde_json::Map<String, Value>, skip: &[&str]) -> String {
    let parts: Vec<String> = data
        .iter()
        .filter(|(key, _)| !skip.contains(&key.as_str()))
        .filter(|(_, val)| val.is_string() || val.is_number() || val.is_boolean())
        .map(|(key, val)| format!("{key}={val}"))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("[{}]", parts.join(", "))
    }
}

fn count(n: i64, label: &str) -> String {
    format!("{n} {label}{}", if n == 1 { "" } else { "es" })
}

struct Frame {
    input: serde_json::Map<String, Value>,
    meta: serde_json::Map<String, Value>,
    state: serde_json::Map<String, Value>,
    status: String,
}

fn frame(part: &Part) -> Frame {
    let meta = part
        .state
        .as_ref()
        .and_then(|s| s.metadata.as_ref())
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Frame {
        input: part
            .state
            .as_ref()
            .and_then(|s| s.input.as_ref())
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        meta,
        state: part
            .state
            .as_ref()
            .map(|s| {
                let mut m = serde_json::Map::new();
                m.insert("status".into(), Value::String(s.status.clone()));
                if let Some(output) = &s.output {
                    m.insert("output".into(), Value::String(output.clone()));
                }
                if let Some(title) = &s.title {
                    m.insert("title".into(), Value::String(title.clone()));
                }
                if let Some(error) = &s.error {
                    m.insert("error".into(), Value::String(error.clone()));
                }
                if let Some(time) = &s.time {
                    m.insert(
                        "time".into(),
                        serde_json::to_value(time).unwrap_or(Value::Null),
                    );
                }
                m
            })
            .unwrap_or_default(),
        status: part
            .state
            .as_ref()
            .map(|s| s.status.clone())
            .unwrap_or_default(),
    }
}

fn fallback_inline(ctx: &Frame, name: &str) -> ToolInline {
    let title = ctx
        .state
        .get("title")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| {
            if ctx.input.is_empty() {
                "Unknown".to_string()
            } else {
                serde_json::to_string(&ctx.input).unwrap_or_default()
            }
        });
    ToolInline {
        icon: "⚙".into(),
        title: format!("{name} {title}"),
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

/// `toolInlineInfo(part)` from tool.ts.
pub fn tool_inline_info(part: &Part) -> ToolInline {
    let ctx = frame(part);
    let name = part.tool.as_deref().unwrap_or("tool");
    run_rule(name, &ctx).unwrap_or_else(|| fallback_inline(&ctx, name))
}

fn run_bash(ctx: &Frame) -> ToolInline {
    let command = ctx
        .input
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let body = if ctx.status == "completed" {
        Some(
            ctx.state
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    ToolInline {
        icon: "$".into(),
        title: command,
        description: None,
        mode: ToolInlineMode::Block,
        body,
    }
}

fn run_glob(ctx: &Frame) -> ToolInline {
    let root = ctx.input.get("path").and_then(Value::as_str).unwrap_or("");
    let title = format!(
        "Glob \"{}\"",
        ctx.input
            .get("pattern")
            .and_then(Value::as_str)
            .unwrap_or("")
    );
    let suffix = if root.is_empty() {
        String::new()
    } else {
        format!("in {}", tool_path(Some(root), false))
    };
    let matches = ctx.meta.get("count").and_then(Value::as_i64);
    let description = match matches {
        Some(matches) => {
            if suffix.is_empty() {
                count(matches, "match")
            } else {
                format!("{suffix} · {}", count(matches, "match"))
            }
        }
        None => suffix,
    };
    ToolInline {
        icon: "✱".into(),
        title,
        description: (!description.is_empty()).then_some(description),
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_grep(ctx: &Frame) -> ToolInline {
    let root = ctx.input.get("path").and_then(Value::as_str).unwrap_or("");
    let title = format!(
        "Grep \"{}\"",
        ctx.input
            .get("pattern")
            .and_then(Value::as_str)
            .unwrap_or("")
    );
    let suffix = if root.is_empty() {
        String::new()
    } else {
        format!("in {}", tool_path(Some(root), false))
    };
    let matches = ctx.meta.get("matches").and_then(Value::as_i64);
    let description = match matches {
        Some(matches) => {
            if suffix.is_empty() {
                count(matches, "match")
            } else {
                format!("{suffix} · {}", count(matches, "match"))
            }
        }
        None => suffix,
    };
    ToolInline {
        icon: "✱".into(),
        title,
        description: (!description.is_empty()).then_some(description),
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_list(ctx: &Frame) -> ToolInline {
    let dir = ctx.input.get("path").and_then(Value::as_str).unwrap_or("");
    ToolInline {
        icon: "→".into(),
        title: if dir.is_empty() {
            "List".to_string()
        } else {
            format!("List {}", tool_path(Some(dir), false))
        },
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_read(ctx: &Frame) -> ToolInline {
    let file = tool_path(ctx.input.get("filePath").and_then(Value::as_str), false);
    let description = info(&ctx.input, &["filePath"]);
    ToolInline {
        icon: "→".into(),
        title: format!("Read {file}"),
        description: (!description.is_empty()).then_some(description),
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_write(ctx: &Frame) -> ToolInline {
    let body = if ctx.status == "completed" {
        Some(
            ctx.state
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )
    } else {
        None
    };
    ToolInline {
        icon: "←".into(),
        title: format!(
            "Write {}",
            tool_path(ctx.input.get("filePath").and_then(Value::as_str), false)
        ),
        description: None,
        mode: ToolInlineMode::Block,
        body,
    }
}

fn run_webfetch(ctx: &Frame) -> ToolInline {
    let url = ctx.input.get("url").and_then(Value::as_str).unwrap_or("");
    ToolInline {
        icon: "%".into(),
        title: if url.is_empty() {
            "WebFetch".to_string()
        } else {
            format!("WebFetch {url}")
        },
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_edit(ctx: &Frame) -> ToolInline {
    ToolInline {
        icon: "←".into(),
        title: format!(
            "Edit {}",
            tool_path(ctx.input.get("filePath").and_then(Value::as_str), false)
        ),
        description: None,
        mode: ToolInlineMode::Block,
        body: ctx
            .meta
            .get("diff")
            .and_then(Value::as_str)
            .map(String::from),
    }
}

fn run_web_search(ctx: &Frame) -> ToolInline {
    let title = web_search_provider_label(ctx.meta.get("provider"));
    let query = ctx.input.get("query").and_then(Value::as_str).unwrap_or("");
    ToolInline {
        icon: "◈".into(),
        title: if query.is_empty() {
            title
        } else {
            format!("{title} \"{query}\"")
        },
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_task(ctx: &Frame) -> ToolInline {
    let kind = titlecase(
        ctx.input
            .get("subagent_type")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
    );
    let desc = ctx
        .input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let icon = match ctx.status.as_str() {
        "error" => "✗",
        "running" => "•",
        _ => "✓",
    };
    let description = if desc.is_empty() {
        None
    } else {
        Some(format!("{kind} Agent"))
    };
    ToolInline {
        icon: icon.into(),
        title: if desc.is_empty() {
            format!("{kind} Task")
        } else {
            desc.to_string()
        },
        description,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_todo(ctx: &Frame) -> ToolInline {
    let body: Vec<String> = list(ctx.input.get("todos"))
        .iter()
        .filter_map(|item| {
            let content = item.get("content").and_then(Value::as_str)?;
            let mark = match item_status(item) {
                Some("completed") => "[✓]",
                Some("in_progress") => "[•]",
                _ => "[ ]",
            };
            Some(format!("{mark} {content}"))
        })
        .collect();
    ToolInline {
        icon: "#".into(),
        title: "Todos".into(),
        description: None,
        mode: ToolInlineMode::Block,
        body: (!body.is_empty()).then_some(body.join("\n")),
    }
}

fn item_status(item: &Value) -> Option<&str> {
    item.get("status").and_then(Value::as_str)
}

fn run_skill(ctx: &Frame) -> ToolInline {
    ToolInline {
        icon: "→".into(),
        title: format!(
            "Skill \"{}\"",
            ctx.input.get("name").and_then(Value::as_str).unwrap_or("")
        ),
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_patch(ctx: &Frame) -> ToolInline {
    let files = ctx
        .meta
        .get("files")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if files == 0 {
        return ToolInline {
            icon: "%".into(),
            title: "Patch".into(),
            description: None,
            mode: ToolInlineMode::Inline,
            body: None,
        };
    }
    ToolInline {
        icon: "%".into(),
        title: format!("Patch {files} file{}", if files == 1 { "" } else { "s" }),
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_question(ctx: &Frame) -> ToolInline {
    let total = list(ctx.input.get("questions")).len();
    ToolInline {
        icon: "→".into(),
        title: format!(
            "Asked {total} question{}",
            if total == 1 { "" } else { "s" }
        ),
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_invalid(ctx: &Frame) -> ToolInline {
    ToolInline {
        icon: "✗".into(),
        title: ctx
            .state
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Invalid Tool")
            .to_string(),
        description: None,
        mode: ToolInlineMode::Block,
        body: (ctx.status == "completed").then(|| {
            ctx.state
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        }),
    }
}

fn run_batch(ctx: &Frame) -> ToolInline {
    let calls = list(ctx.input.get("tool_calls")).len();
    let title = ctx
        .state
        .get("title")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| {
            if calls > 0 {
                format!("Batch {calls} tool{}", if calls == 1 { "" } else { "s" })
            } else {
                "Batch".to_string()
            }
        });
    ToolInline {
        icon: "#".into(),
        title,
        description: None,
        mode: ToolInlineMode::Block,
        body: (ctx.status == "completed").then(|| {
            ctx.state
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        }),
    }
}

fn lsp_title(input: &serde_json::Map<String, Value>) -> String {
    let op = input
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("request");
    let file = input
        .get("filePath")
        .and_then(Value::as_str)
        .map(|f| tool_path(Some(f), false))
        .unwrap_or_default();
    let line = input.get("line").and_then(Value::as_i64);
    let character = input.get("character").and_then(Value::as_i64);
    let pos = match (line, character) {
        (Some(line), Some(character)) => format!(":{line}:{character}"),
        _ => String::new(),
    };
    if file.is_empty() {
        format!("LSP {op}")
    } else {
        format!("LSP {op} {file}{pos}")
    }
}

fn run_lsp(ctx: &Frame) -> ToolInline {
    ToolInline {
        icon: "→".into(),
        title: ctx
            .state
            .get("title")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| lsp_title(&ctx.input)),
        description: None,
        mode: ToolInlineMode::Inline,
        body: None,
    }
}

fn run_plan_exit(ctx: &Frame) -> ToolInline {
    ToolInline {
        icon: "→".into(),
        title: ctx
            .state
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Switching to build agent")
            .to_string(),
        description: None,
        mode: ToolInlineMode::Block,
        body: (ctx.status == "completed").then(|| {
            ctx.state
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        }),
    }
}

fn run_rule(name: &str, ctx: &Frame) -> Option<ToolInline> {
    match name {
        "bash" => Some(run_bash(ctx)),
        "glob" => Some(run_glob(ctx)),
        "grep" => Some(run_grep(ctx)),
        "list" => Some(run_list(ctx)),
        "read" => Some(run_read(ctx)),
        "write" => Some(run_write(ctx)),
        "webfetch" => Some(run_webfetch(ctx)),
        "edit" => Some(run_edit(ctx)),
        "websearch" => Some(run_web_search(ctx)),
        "task" => Some(run_task(ctx)),
        "todowrite" => Some(run_todo(ctx)),
        "skill" => Some(run_skill(ctx)),
        "apply_patch" | "patch" => Some(run_patch(ctx)),
        "question" => Some(run_question(ctx)),
        "invalid" => Some(run_invalid(ctx)),
        "batch" => Some(run_batch(ctx)),
        "lsp" => Some(run_lsp(ctx)),
        "plan_exit" => Some(run_plan_exit(ctx)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(tool: &str, state: Value) -> Part {
        serde_json::from_value(serde_json::json!({
            "id": "prt_1", "sessionID": "ses_1", "messageID": "msg_1",
            "type": "tool", "callID": "call_1", "tool": tool, "state": state
        }))
        .unwrap()
    }

    #[test]
    fn bash_completed_renders_output_block() {
        let p = part(
            "bash",
            serde_json::json!({
                "status": "completed",
                "input": {"command": "ls -la"},
                "output": "total 0\n",
                "title": "bash",
                "time": {"start": 1000, "end": 1500}
            }),
        );
        let info = tool_inline_info(&p);
        assert_eq!(info.icon, "$");
        assert_eq!(info.title, "ls -la");
        assert_eq!(info.mode, ToolInlineMode::Block);
        assert_eq!(info.body.as_deref(), Some("total 0"));
    }

    #[test]
    fn task_running_uses_bullet() {
        let p = part(
            "task",
            serde_json::json!({
                "status": "running",
                "input": {"subagent_type": "general", "description": "Fix the bug"},
                "time": {"start": 1}
            }),
        );
        let info = tool_inline_info(&p);
        assert_eq!(info.icon, "•");
        assert_eq!(info.title, "Fix the bug");
        assert_eq!(info.description.as_deref(), Some("General Agent"));
    }

    #[test]
    fn unknown_tool_falls_back() {
        let p = part("unknown-tool", serde_json::json!({"status": "completed"}));
        let info = tool_inline_info(&p);
        assert_eq!(info.icon, "⚙");
        assert!(info.title.starts_with("unknown-tool"));
    }

    #[test]
    fn glob_counts_matches() {
        let p = serde_json::from_value(serde_json::json!({
            "id": "prt_1", "sessionID": "ses_1", "messageID": "msg_1",
            "type": "tool", "callID": "call_1", "tool": "glob",
            "state": {"status": "completed", "input": {"pattern": "**/*.rs"}, "metadata": {"count": 3}, "time": {"start": 1, "end": 2}}
        }))
        .unwrap();
        let info = tool_inline_info(&p);
        assert_eq!(info.icon, "✱");
        assert_eq!(info.title, "Glob \"**/*.rs\"");
        assert_eq!(info.description.as_deref(), Some("3 matches"));
    }

    #[test]
    fn titlecase_capitalizes_words() {
        assert_eq!(titlecase("general purpose"), "General Purpose");
    }
}
