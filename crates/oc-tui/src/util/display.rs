//! Pure display helpers for rendering session parts.
//!
//! Mirrors `reference/packages/tui/src/routes/session/index.tsx` helper
//! functions, `util/tool-display.ts`, `util/session.ts` and
//! `context/thinking.ts`.

use serde_json::Value;

/// Map a tool name to its display variant; unknown tools render generically.
/// From reference/packages/tui/src/routes/session/index.tsx (`toolDisplay`)
pub fn tool_display(tool: &str) -> &str {
    const KNOWN: &[&str] = &[
        "bash",
        "glob",
        "read",
        "grep",
        "webfetch",
        "websearch",
        "write",
        "edit",
        "task",
        "apply_patch",
        "todowrite",
        "question",
        "skill",
        "execute",
    ];
    if KNOWN.contains(&tool) {
        tool
    } else {
        "generic"
    }
}

pub fn web_search_provider_label(provider: &Value) -> &'static str {
    match provider.as_str() {
        Some("parallel") => "Parallel Web Search",
        Some("exa") => "Exa Web Search",
        _ => "Web Search",
    }
}

pub fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(|s| s.to_string())
}

pub fn number_value(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64)
}

pub fn record_value(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Render primitive tool-call args as `[key=value, ...]`.
/// From reference/packages/tui/src/routes/session/index.tsx (`input`)
pub fn format_tool_input(input: &serde_json::Map<String, Value>, omit: &[&str]) -> String {
    let primitives: Vec<(String, String)> = input
        .iter()
        .filter(|(k, _)| !omit.contains(&k.as_str()))
        .filter_map(|(k, v)| {
            let rendered = match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            };
            rendered.map(|r| (k.clone(), r))
        })
        .collect();
    if primitives.is_empty() {
        return String::new();
    }
    let joined: Vec<String> = primitives
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    format!("[{}]", joined.join(", "))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyPatchFile {
    pub type_: String,
    pub relative_path: String,
    pub file_path: String,
    pub patch: String,
    pub deletions: i64,
    pub move_path: Option<String>,
}

/// Parse the `files` metadata of an apply_patch tool.
/// From reference/packages/tui/src/routes/session/index.tsx (`parseApplyPatchFiles`)
pub fn parse_apply_patch_files(value: &Value) -> Vec<ApplyPatchFile> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let map = record_value(item)?;
            let type_ = string_value(map.get("type"))?;
            let relative_path = string_value(map.get("relativePath"))?;
            let file_path = string_value(map.get("filePath"))?;
            let patch = string_value(map.get("patch"))?;
            let deletions = number_value(map.get("deletions"))?;
            let move_path = map.get("movePath").and_then(|v| string_value(Some(v)));
            Some(ApplyPatchFile {
                type_,
                relative_path,
                file_path,
                patch,
                deletions,
                move_path,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub status: String,
    pub content: String,
}

/// From reference/packages/tui/src/routes/session/index.tsx (`parseTodos`)
pub fn parse_todos(value: &Value) -> Vec<TodoItem> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let map = record_value(item)?;
            let status = string_value(map.get("status"))?;
            let content = string_value(map.get("content"))?;
            Some(TodoItem { status, content })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub question: String,
}

/// From reference/packages/tui/src/routes/session/index.tsx (`parseQuestions`)
pub fn parse_questions(value: &Value) -> Vec<Question> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let map = record_value(item)?;
            let question = string_value(map.get("question"))?;
            Some(Question { question })
        })
        .collect()
}

/// From reference/packages/tui/src/routes/session/index.tsx (`parseQuestionAnswers`)
pub fn parse_question_answers(value: &Value) -> Option<Vec<Vec<String>>> {
    let items = value.as_array()?;
    Some(
        items
            .iter()
            .map(|a| {
                a.as_array()
                    .map(|inner| inner.iter().filter_map(|v| string_value(Some(v))).collect())
                    .unwrap_or_default()
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: i64,
    pub character: i64,
    pub message: String,
}

/// Parse LSP diagnostics keyed by file path (severity 1 == error).
/// From reference/packages/tui/src/routes/session/index.tsx (`parseDiagnostics`)
pub fn parse_diagnostics(value: &Value, file_path: &str) -> Vec<Diagnostic> {
    let Some(map) = record_value(value) else {
        return Vec::new();
    };
    let Some(items) = map.get(file_path).and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut result: Vec<Diagnostic> = Vec::new();
    for item in items {
        let Some(diag) = record_value(item) else {
            continue;
        };
        if diag.get("severity").and_then(Value::as_u64) != Some(1) {
            continue;
        }
        let Some(start) = diag
            .get("range")
            .and_then(record_value)
            .and_then(|r| r.get("start"))
            .and_then(record_value)
        else {
            continue;
        };
        let Some(line) = start.get("line").and_then(Value::as_i64) else {
            continue;
        };
        let Some(character) = start.get("character").and_then(Value::as_i64) else {
            continue;
        };
        let Some(message) = diag.get("message").and_then(Value::as_str) else {
            continue;
        };
        result.push(Diagnostic {
            line,
            character,
            message: message.to_string(),
        });
        if result.len() >= 3 {
            break;
        }
    }
    result
}

/// Parse `execute` tool child calls streamed through `metadata.toolCalls`.
/// From reference/packages/tui/src/routes/session/index.tsx (`executeCalls`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteCall {
    pub tool: String,
    pub status: String,
    pub input: Option<serde_json::Map<String, Value>>,
}

pub fn parse_execute_calls(value: &Value) -> Vec<ExecuteCall> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let map = record_value(item)?;
            let tool = string_value(map.get("tool"))?;
            let status = string_value(map.get("status"))?;
            if !["running", "completed", "error"].contains(&status.as_str()) {
                return None;
            }
            let input = map.get("input").and_then(record_value).cloned();
            Some(ExecuteCall {
                tool,
                status,
                input,
            })
        })
        .collect()
}

pub fn format_subagent_toolcalls(count: usize) -> String {
    if count == 1 {
        "1 toolcall".to_string()
    } else {
        format!("{count} toolcalls")
    }
}

pub fn format_subagent_title(agent: &str, description: &str, background: bool) -> String {
    if background {
        format!("{agent} Task (background) — {description}")
    } else {
        format!("{agent} Task — {description}")
    }
}

pub fn format_subagent_retry(attempt: u64, message: &str) -> String {
    format!("Retrying (attempt {attempt}) · {message}")
}

pub fn format_completed_subagent_detail(toolcalls: usize, duration: &str) -> String {
    if toolcalls == 0 {
        duration.to_string()
    } else {
        format!("{} · {duration}", format_subagent_toolcalls(toolcalls))
    }
}

/// Extract a bolded title block from OpenAI-style reasoning summaries.
/// From reference/packages/tui/src/context/thinking.ts (`reasoningSummary`)
pub fn reasoning_summary(text: &str) -> (Option<String>, String) {
    let content = text.trim();
    let re = regex::Regex::new(r"^\*\*([^*\n]+)\*\*(?:\r?\n\r?\n|$)").expect("static regex");
    match re.captures(content) {
        Some(caps) => {
            let title = caps.get(1).map(|m| m.as_str().trim().to_string());
            let body = content[caps.get(0).unwrap().len()..].trim_end().to_string();
            (title, body)
        }
        None => (None, content.to_string()),
    }
}

/// Cycle thinking visibility: show → hide → show.
/// From reference/packages/tui/src/context/thinking.ts (`nextThinkingMode`)
pub fn next_thinking_mode(current: &str) -> &'static str {
    if current == "show" {
        "hide"
    } else {
        "show"
    }
}

/// Session default title detection.
/// From reference/packages/tui/src/util/session.ts (`isDefaultTitle`)
pub fn is_default_title(title: &str) -> bool {
    let re = regex::Regex::new(
        r"^(New session - |Child session - )\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$",
    )
    .expect("static regex");
    re.is_match(title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_input_formats_primitives() {
        let input = serde_json::from_value(json!({
            "command": "ls",
            "workdir": ".",
            "count": 3,
            "replaceAll": true,
            "nested": { "a": 1 }
        }))
        .unwrap();
        let s = format_tool_input(&input, &[]);
        assert!(s.contains("command=ls"));
        assert!(s.contains("count=3"));
        assert!(s.contains("replaceAll=true"));
        assert!(!s.contains("nested"));
    }

    #[test]
    fn tool_input_omits_keys() {
        let input = serde_json::from_value(json!({
            "filePath": "src/a.ts",
            "content": "x"
        }))
        .unwrap();
        let s = format_tool_input(&input, &["filePath"]);
        assert!(!s.contains("filePath"));
        assert!(s.contains("content=x"));
    }

    #[test]
    fn parse_apply_patch_files_filters_invalid() {
        let value = json!([
            { "type": "add", "relativePath": "a.txt", "filePath": "a.txt", "patch": "x", "deletions": 0 },
            { "type": "bad" }
        ]);
        let files = parse_apply_patch_files(&value);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "a.txt");
    }

    #[test]
    fn parse_todos_and_questions() {
        let todos = parse_todos(
            &json!([{ "status": "in_progress", "content": "write code" }, { "status": "x" }]),
        );
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].status, "in_progress");

        let qs = parse_questions(&json!([{ "question": "why?" }, { "foo": 1 }]));
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].question, "why?");

        let answers = parse_question_answers(&json!([["a"], [], ["b", "c"]]));
        assert_eq!(
            answers,
            Some(vec![
                vec!["a".to_string()],
                vec![],
                vec!["b".to_string(), "c".to_string()]
            ])
        );
    }

    #[test]
    fn parse_diagnostics_filters_severity() {
        let value = json!({
            "src/a.ts": [
                { "severity": 1, "range": { "start": { "line": 3, "character": 1 } }, "message": "err1" },
                { "severity": 2, "range": { "start": { "line": 9, "character": 0 } }, "message": "warn" },
                { "severity": 1, "range": { "start": { "line": 5, "character": 2 } }, "message": "err2" }
            ]
        });
        let d = parse_diagnostics(&value, "src/a.ts");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].message, "err1");
        assert_eq!(d[0].line, 3);
    }

    #[test]
    fn reasoning_summary_handles_title_block() {
        let (title, body) = reasoning_summary("**Inspecting PR workflow**\n\nsome body");
        assert_eq!(title.as_deref(), Some("Inspecting PR workflow"));
        assert_eq!(body, "some body");

        let (title, body) = reasoning_summary("plain reasoning");
        assert_eq!(title, None);
        assert_eq!(body, "plain reasoning");
    }

    #[test]
    fn execute_calls_parse() {
        let calls = parse_execute_calls(&json!([
            { "tool": "read", "status": "completed", "input": { "filePath": "a" } },
            { "tool": "nope", "status": "bogus" },
            { "tool": "bash", "status": "running" }
        ]));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].tool, "read");
        assert!(calls[0].input.is_some());
    }

    #[test]
    fn subagent_formatters() {
        assert_eq!(format_subagent_toolcalls(1), "1 toolcall");
        assert_eq!(format_subagent_toolcalls(2), "2 toolcalls");
        assert_eq!(
            format_subagent_title("Build", "fix the thing", false),
            "Build Task — fix the thing"
        );
        assert_eq!(
            format_subagent_title("Build", "fix", true),
            "Build Task (background) — fix"
        );
        assert_eq!(
            format_subagent_retry(2, "failed"),
            "Retrying (attempt 2) · failed"
        );
        assert_eq!(format_completed_subagent_detail(0, "5s"), "5s");
        assert_eq!(
            format_completed_subagent_detail(3, "5s"),
            "3 toolcalls · 5s"
        );
    }

    #[test]
    fn default_title_detection() {
        assert!(is_default_title("New session - 2026-01-01T00:00:00.000Z"));
        assert!(is_default_title("Child session - 2026-01-01T00:00:00.000Z"));
        assert!(!is_default_title("Custom title"));
    }
}
