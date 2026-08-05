//! The `run` event loop: mirrors the `loop()` function in
//! reference/packages/opencode/src/cli/cmd/run.ts. Streams session events and
//! mirrors them to stdout / the CLI UI.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::io::Write;

use futures::{Stream, StreamExt};

use super::client::RunClient;
use super::tool;
use super::types::{GlobalEvent, MessageInfo, Part, PermissionRequest, SessionStatus};
use crate::cli::ui::{self, Style};

pub struct LoopOptions {
    pub format_json: bool,
    pub thinking: bool,
    pub auto: bool,
    pub session_id: String,
}

/// Whether `--format json` is active: writes `{type,timestamp,sessionID,...}`
/// envelopes to stdout, mirroring the reference `emit()` helper.
fn emit(
    format_json: bool,
    event_type: &str,
    extra: &serde_json::Map<String, serde_json::Value>,
    session_id: &str,
) -> bool {
    if !format_json {
        return false;
    }
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), event_type.into());
    obj.insert(
        "timestamp".into(),
        (chrono::Utc::now().timestamp_millis()).into(),
    );
    obj.insert("sessionID".into(), session_id.into());
    for (key, value) in extra {
        obj.insert(key.clone(), value.clone());
    }
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{}", serde_json::Value::Object(obj));
    true
}

async fn tool(part: &Part) {
    let next = tool::tool_inline_info(part);
    if next.mode == tool::ToolInlineMode::Block {
        block(&next);
        return;
    }
    inline(&next);
}

async fn tool_error(part: &Part) {
    let next = tool::tool_inline_info(part);
    let title = format!("{} failed", next.title);
    ui::println(&["✗", Style::TEXT_NORMAL, &title]);
}

fn inline(info: &tool::ToolInline) {
    let combined = match &info.description {
        Some(description) => {
            format!("{}{} {}", info.title, Style::TEXT_DIM, description) + Style::TEXT_NORMAL
        }
        None => info.title.clone(),
    };
    ui::println(&[&info.icon, Style::TEXT_NORMAL, &combined]);
}

fn block(info: &tool::ToolInline) {
    ui::empty();
    inline(info);
    let output = info.body.as_deref().unwrap_or("");
    if output.trim().is_empty() {
        return;
    }
    ui::println(&[output]);
    ui::empty();
}

fn write_stdout(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.write_all(b"\n");
}

/// Run the event loop until the session goes idle. Returns the accumulated
/// session error (if any), mirroring the reference `loop()` result.
pub async fn event_loop<S, E>(
    client: &dyn RunClient,
    stream: S,
    opts: &LoopOptions,
) -> anyhow::Result<Option<String>>
where
    S: Stream<Item = Result<GlobalEvent, E>> + Unpin,
    E: Into<anyhow::Error>,
{
    let mut toggles: HashMap<String, bool> = HashMap::new();
    let mut error: Option<String> = None;
    let session_id = &opts.session_id;

    let mut stream = stream.map(|item| item.map_err(Into::into));
    while let Some(event) = stream.next().await {
        let event = event?;
        match event.event_type.as_str() {
            "message.updated" => {
                let properties = &event.properties;
                let event_session_id = properties
                    .get("sessionID")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let info: MessageInfo = serde_json::from_value(
                    properties
                        .get("info")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .unwrap_or_default();
                if event_session_id == *session_id
                    && info.role == "assistant"
                    && !opts.format_json
                    && toggles.get("start").copied() != Some(true)
                {
                    let agent = info.agent.as_deref().unwrap_or("");
                    let model = info.model_id.as_deref().unwrap_or("");
                    ui::empty();
                    ui::println(&[&format!("> {agent} · {model}")]);
                    ui::empty();
                    toggles.insert("start".into(), true);
                }
            }
            "message.part.updated" => {
                let properties = &event.properties;
                let part: Part = serde_json::from_value(
                    properties
                        .get("part")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .unwrap_or_default();
                if part.session_id.is_empty() {}
                if part.session_id != *session_id {
                    continue;
                }

                match part.part_type.as_str() {
                    "tool" => {
                        let status = part.state.as_ref().map(|s| s.status.as_str()).unwrap_or("");
                        if status == "completed" || status == "error" {
                            let mut extra = serde_json::Map::new();
                            extra.insert(
                                "part".into(),
                                serde_json::to_value(&part).unwrap_or(serde_json::Value::Null),
                            );
                            if emit(opts.format_json, "tool_use", &extra, session_id) {
                                continue;
                            }
                            if status == "completed" {
                                tool(&part).await;
                                continue;
                            }
                            tool_error(&part).await;
                            let err = part
                                .state
                                .as_ref()
                                .and_then(|s| s.error.as_deref())
                                .unwrap_or_default();
                            ui::error(err);
                        }

                        if part.tool.as_deref() == Some("task")
                            && status == "running"
                            && !opts.format_json
                        {
                            if toggles.get(&part.id).copied() == Some(true) {
                                continue;
                            }
                            tool(&part).await;
                            toggles.insert(part.id.clone(), true);
                        }
                    }
                    "step-start" => {
                        let mut extra = serde_json::Map::new();
                        extra.insert(
                            "part".into(),
                            serde_json::to_value(&part).unwrap_or_default(),
                        );
                        emit(opts.format_json, "step_start", &extra, session_id);
                    }
                    "step-finish" => {
                        let mut extra = serde_json::Map::new();
                        extra.insert(
                            "part".into(),
                            serde_json::to_value(&part).unwrap_or_default(),
                        );
                        emit(opts.format_json, "step_finish", &extra, session_id);
                    }
                    "text" => {
                        if part.time.as_ref().and_then(|t| t.end).is_none() {
                            continue;
                        }
                        let mut extra = serde_json::Map::new();
                        extra.insert(
                            "part".into(),
                            serde_json::to_value(&part).unwrap_or_default(),
                        );
                        if emit(opts.format_json, "text", &extra, session_id) {
                            continue;
                        }
                        let text = part.text.as_deref().unwrap_or("").trim().to_string();
                        if text.is_empty() {
                            continue;
                        }
                        if !std::io::stdout().is_terminal() {
                            write_stdout(&text);
                            continue;
                        }
                        ui::empty();
                        ui::println(&[&text]);
                        ui::empty();
                    }
                    "reasoning" => {
                        if part.time.as_ref().and_then(|t| t.end).is_none() || !opts.thinking {
                            continue;
                        }
                        let mut extra = serde_json::Map::new();
                        extra.insert(
                            "part".into(),
                            serde_json::to_value(&part).unwrap_or_default(),
                        );
                        if emit(opts.format_json, "reasoning", &extra, session_id) {
                            continue;
                        }
                        let text = part.text.as_deref().unwrap_or("").trim().to_string();
                        if text.is_empty() {
                            continue;
                        }
                        let line = format!("Thinking: {text}");
                        if std::io::stdout().is_terminal() {
                            ui::empty();
                            ui::println(&[
                                &format!("{}{}{}", Style::TEXT_DIM, "\x1b[3m", line),
                                "\x1b[0m",
                                Style::TEXT_NORMAL,
                            ]);
                            ui::empty();
                            continue;
                        }
                        write_stdout(&line);
                    }
                    _ => {}
                }
            }
            "session.error" => {
                let properties = &event.properties;
                if properties
                    .get("sessionID")
                    .and_then(serde_json::Value::as_str)
                    != Some(session_id)
                {
                    continue;
                }
                let err_value = properties.get("error");
                if err_value.map_or(true, serde_json::Value::is_null) {
                    continue;
                }
                let err = session_error_message(err_value);
                error = Some(match error {
                    Some(prev) => format!("{prev}\n{err}"),
                    None => err.clone(),
                });
                let mut extra = serde_json::Map::new();
                extra.insert("error".into(), err_value.cloned().unwrap_or_default());
                if emit(opts.format_json, "error", &extra, session_id) {
                    continue;
                }
                ui::error(&err);
            }
            "session.status" => {
                let properties = &event.properties;
                if properties
                    .get("sessionID")
                    .and_then(serde_json::Value::as_str)
                    == Some(session_id)
                {
                    let status: SessionStatus = serde_json::from_value(
                        properties.get("status").cloned().unwrap_or_default(),
                    )
                    .unwrap_or(SessionStatus {
                        status_type: String::new(),
                    });
                    if status.status_type == "idle" {
                        break;
                    }
                }
            }
            "permission.asked" => {
                let properties = &event.properties;
                let permission: PermissionRequest =
                    serde_json::from_value(properties.clone()).unwrap_or_default();
                if permission.session_id != *session_id {
                    continue;
                }
                if opts.auto {
                    let _ = client
                        .permission_reply(permission.id.clone(), "once".to_string())
                        .await;
                } else {
                    ui::println(&[
                        Style::TEXT_WARNING_BOLD,
                        "!",
                        Style::TEXT_NORMAL,
                        &format!(
                            "permission requested: {} ({}); auto-rejecting",
                            permission.permission,
                            permission.patterns.join(", ")
                        ),
                    ]);
                    let _ = client
                        .permission_reply(permission.id.clone(), "reject".to_string())
                        .await;
                }
            }
            _ => {}
        }
    }
    Ok(error)
}

fn session_error_message(err_value: Option<&serde_json::Value>) -> String {
    let err = err_value.unwrap_or(&serde_json::Value::Null);
    let name = err
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if let Some(message) = err
        .get("data")
        .and_then(|data| data.get("message"))
        .and_then(serde_json::Value::as_str)
    {
        message.to_string()
    } else {
        name.to_string()
    }
}
