/// From reference/packages/opencode/src/session/message-v2.ts
///
/// Message history helpers: paging cursors, `WithParts` → model message
/// conversion, compaction filtering and error mapping.
use crate::llm::{UiFilePart, UiReasoningPart, UiTextPart};
use crate::provider::ProviderModel;
use crate::util;
use crate::v1::{Error as V1Error, Info, Part, WithParts};

pub const SYNTHETIC_ATTACHMENT_PROMPT: &str = "Attached media from tool result:";

pub fn is_media(mime: &str) -> bool {
    mime.starts_with("image/") || mime == "application/pdf"
}

fn truncate_tool_output(text: &str, max_chars: Option<usize>) -> String {
    match max_chars {
        Some(max) if text.chars().count() > max => {
            let chars: Vec<char> = text.chars().collect();
            let omitted = chars.len() - max;
            format!(
                "{}\n[Tool output truncated for compaction: omitted {omitted} chars]",
                chars[..max].iter().collect::<String>()
            )
        }
        _ => text.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    pub id: String,
    pub time: f64,
}

/// From reference `message-v2.ts:cursor` — base64url JSON cursor.
pub mod cursor {
    use super::*;

    pub fn encode(input: &Cursor) -> String {
        let json = format!("{{\"id\":\"{}\",\"time\":{}}}", input.id, input.time);
        util::base64url_encode(json.as_bytes())
    }

    pub fn decode(input: &str) -> Option<Cursor> {
        let bytes = util::base64url_decode(input)?;
        let text = String::from_utf8(bytes).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        Some(Cursor {
            id: value.get("id")?.as_str()?.to_string(),
            time: value.get("time")?.as_f64()?,
        })
    }
}

/// From reference `message-v2.ts:providerMeta` — strips `providerExecuted`.
fn provider_meta(metadata: Option<&crate::JsonMap>) -> Option<crate::JsonMap> {
    metadata.and_then(|metadata| {
        let rest: crate::JsonMap = metadata
            .iter()
            .filter(|(k, _)| k.as_str() != "providerExecuted")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if rest.is_empty() {
            None
        } else {
            Some(rest)
        }
    })
}

/// AI SDK `UIMessage` shape produced by `to_model_messages`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMessage {
    pub id: String,
    pub role: String,
    pub parts: Vec<UiPart>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum UiPart {
    Text(UiTextPart),
    File(UiFilePart),
    StepStart(UiStepStartPart),
    Reasoning(UiReasoningPart),
    Tool(UiToolPart),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiStepStartPart {
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiToolPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub state: String,
    pub tool_call_id: String,
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_provider_metadata: Option<crate::JsonMap>,
}

/// From reference `message-v2.ts:toModelMessages` (UIMessage layer). The final
/// `convertToModelMessages` lowering is provided by `to_model_messages`.
pub fn to_model_messages(
    input: &[WithParts],
    model: &ProviderModel,
    options: &ToModelOptions,
) -> Vec<UiMessage> {
    let mut result: Vec<UiMessage> = Vec::new();
    let mut tool_names = std::collections::HashSet::new();
    let supports_media_in_tool_result = |attachment: &UiFilePart| {
        let npm = model.api.npm.as_deref().unwrap_or("");
        if npm == "@ai-sdk/anthropic"
            || npm == "@ai-sdk/openai"
            || npm == "@ai-sdk/amazon-bedrock/mantle"
            || npm == "@ai-sdk/google-vertex/anthropic"
        {
            return true;
        }
        if npm == "@ai-sdk/amazon-bedrock" || npm == "@ai-sdk/xai" {
            return attachment.media_type.starts_with("image/");
        }
        if npm == "@ai-sdk/google" {
            let id = model.api.id.to_lowercase();
            return id.contains("gemini-3") && !id.contains("gemini-2");
        }
        false
    };

    for msg in input {
        if msg.parts.is_empty() {
            continue;
        }
        match &msg.info {
            Info::User(user) => {
                let mut user_message = UiMessage {
                    id: user.id.clone(),
                    role: "user".to_string(),
                    parts: Vec::new(),
                };
                for part in &msg.parts {
                    match part {
                        Part::Text(text) => {
                            if !text.ignored.unwrap_or(false) && !text.text.is_empty() {
                                user_message.parts.push(UiPart::Text(UiTextPart {
                                    type_: "text".into(),
                                    text: text.text.clone(),
                                    provider_metadata: None,
                                }));
                            }
                        }
                        Part::File(file) => {
                            if file.mime != "text/plain" && file.mime != "application/x-directory" {
                                if options.strip_media && is_media(&file.mime) {
                                    user_message.parts.push(UiPart::Text(UiTextPart {
                                        type_: "text".into(),
                                        text: format!(
                                            "[Attached {}: {}]",
                                            file.mime,
                                            file.filename
                                                .clone()
                                                .unwrap_or_else(|| "file".to_string())
                                        ),
                                        provider_metadata: None,
                                    }));
                                } else {
                                    user_message.parts.push(UiPart::File(UiFilePart {
                                        type_: "file".into(),
                                        url: file.url.clone(),
                                        media_type: file.mime.clone(),
                                        filename: file.filename.clone(),
                                    }));
                                }
                            }
                        }
                        Part::Compaction(_) => {
                            user_message.parts.push(UiPart::Text(UiTextPart {
                                type_: "text".into(),
                                text: "What did we do so far?".into(),
                                provider_metadata: None,
                            }));
                        }
                        Part::Subtask(_) => {
                            user_message.parts.push(UiPart::Text(UiTextPart {
                                type_: "text".into(),
                                text: "The following tool was executed by the user".into(),
                                provider_metadata: None,
                            }));
                        }
                        _ => {}
                    }
                }
                if !user_message.parts.is_empty() {
                    result.push(user_message);
                }
            }
            Info::Assistant(assistant) => {
                let different_model = format!("{}/{}", model.provider_id, model.id)
                    != format!("{}/{}", assistant.provider_id, assistant.model_id);
                let mut media: Vec<UiFilePart> = Vec::new();

                // Skip errored assistant turns unless aborted with no visible content.
                if let Some(error) = &assistant.error {
                    let is_aborted_without_content = matches!(error, V1Error::AbortedError { .. })
                        && !msg
                            .parts
                            .iter()
                            .any(|part| !matches!(part, Part::StepStart(_) | Part::Reasoning(_)));
                    if !is_aborted_without_content {
                        continue;
                    }
                }
                let mut assistant_message = UiMessage {
                    id: assistant.id.clone(),
                    role: "assistant".to_string(),
                    parts: Vec::new(),
                };
                let has_signed_reasoning = msg.parts.iter().any(|part| {
                    matches!(part, Part::Reasoning(reasoning) if reasoning.metadata.as_ref().is_some_and(|m| m.get("anthropic").and_then(|a| a.get("signature")).is_some()))
                });
                for part in &msg.parts {
                    match part {
                        Part::Text(text) => {
                            let text_value = if text.text.is_empty() && has_signed_reasoning {
                                " ".to_string()
                            } else {
                                text.text.clone()
                            };
                            let mut ui_text = UiTextPart {
                                type_: "text".into(),
                                text: text_value,
                                provider_metadata: None,
                            };
                            if !different_model {
                                ui_text.provider_metadata = text.metadata.clone();
                            }
                            assistant_message.parts.push(UiPart::Text(ui_text));
                        }
                        Part::StepStart(_) => {
                            assistant_message
                                .parts
                                .push(UiPart::StepStart(UiStepStartPart {
                                    type_: "step-start".into(),
                                }));
                        }
                        Part::Tool(tool) => {
                            tool_names.insert(tool.tool.clone());
                            match &tool.state {
                                crate::v1::ToolState::Completed(state) => {
                                    let output_text = if state.time.compacted.is_some() {
                                        "[Old tool result content cleared]".to_string()
                                    } else {
                                        truncate_tool_output(
                                            &state.output,
                                            options.tool_output_max_chars,
                                        )
                                    };
                                    let attachments: Vec<UiFilePart> =
                                        if state.time.compacted.is_some() || options.strip_media {
                                            Vec::new()
                                        } else {
                                            state
                                                .attachments
                                                .clone()
                                                .unwrap_or_default()
                                                .iter()
                                                .map(|a| UiFilePart {
                                                    type_: "file".into(),
                                                    url: a.url.clone(),
                                                    media_type: a.mime.clone(),
                                                    filename: a.filename.clone(),
                                                })
                                                .collect()
                                        };
                                    let media_attachments: Vec<UiFilePart> = attachments
                                        .iter()
                                        .filter(|a| is_media(&a.media_type))
                                        .cloned()
                                        .collect();
                                    let extracted: Vec<UiFilePart> = media_attachments
                                        .iter()
                                        .filter(|a| !supports_media_in_tool_result(a))
                                        .cloned()
                                        .collect();
                                    media.extend(extracted);
                                    let final_attachments: Vec<UiFilePart> = attachments
                                        .iter()
                                        .filter(|a| {
                                            !is_media(&a.media_type)
                                                || supports_media_in_tool_result(a)
                                        })
                                        .cloned()
                                        .collect();

                                    let output = if final_attachments.is_empty() {
                                        serde_json::Value::String(output_text)
                                    } else {
                                        serde_json::json!({
                                            "text": output_text,
                                            "attachments": final_attachments.iter().map(|a| serde_json::json!({
                                                "mime": a.media_type,
                                                "url": a.url
                                            })).collect::<Vec<_>>()
                                        })
                                    };

                                    let mut part_value = serde_json::json!({
                                        "type": format!("tool-{}", tool.tool),
                                        "state": "output-available",
                                        "toolCallId": tool.call_id,
                                        "input": state.input,
                                        "output": output
                                    });
                                    if tool.metadata.as_ref().is_some_and(|m| {
                                        m.get("providerExecuted")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false)
                                    }) {
                                        part_value["providerExecuted"] =
                                            serde_json::Value::Bool(true);
                                    }
                                    if !different_model {
                                        if let Some(meta) = provider_meta(tool.metadata.as_ref()) {
                                            part_value["callProviderMetadata"] =
                                                serde_json::to_value(meta).unwrap();
                                        }
                                    }
                                    push_part_value(&mut assistant_message, part_value);
                                }
                                crate::v1::ToolState::Error(state) => {
                                    let interrupted_output = state
                                        .metadata
                                        .as_ref()
                                        .and_then(|m| {
                                            if m.get("interrupted")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false)
                                            {
                                                m.get("output").cloned()
                                            } else {
                                                None
                                            }
                                        })
                                        .and_then(|v| v.as_str().map(|s| s.to_string()));
                                    if let Some(output) = interrupted_output {
                                        let mut part_value = serde_json::json!({
                                            "type": format!("tool-{}", tool.tool),
                                            "state": "output-available",
                                            "toolCallId": tool.call_id,
                                            "input": state.input,
                                            "output": output
                                        });
                                        if tool.metadata.as_ref().is_some_and(|m| {
                                            m.get("providerExecuted")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false)
                                        }) {
                                            part_value["providerExecuted"] =
                                                serde_json::Value::Bool(true);
                                        }
                                        if !different_model {
                                            if let Some(meta) =
                                                provider_meta(tool.metadata.as_ref())
                                            {
                                                part_value["callProviderMetadata"] =
                                                    serde_json::to_value(meta).unwrap();
                                            }
                                        }
                                        push_part_value(&mut assistant_message, part_value);
                                    } else {
                                        let mut part_value = serde_json::json!({
                                            "type": format!("tool-{}", tool.tool),
                                            "state": "output-error",
                                            "toolCallId": tool.call_id,
                                            "input": state.input,
                                            "errorText": state.error
                                        });
                                        if tool.metadata.as_ref().is_some_and(|m| {
                                            m.get("providerExecuted")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false)
                                        }) {
                                            part_value["providerExecuted"] =
                                                serde_json::Value::Bool(true);
                                        }
                                        if !different_model {
                                            if let Some(meta) =
                                                provider_meta(tool.metadata.as_ref())
                                            {
                                                part_value["callProviderMetadata"] =
                                                    serde_json::to_value(meta).unwrap();
                                            }
                                        }
                                        push_part_value(&mut assistant_message, part_value);
                                    }
                                }
                                crate::v1::ToolState::Pending(pending) => {
                                    let input = pending.input.clone();
                                    let mut part_value = serde_json::json!({
                                        "type": format!("tool-{}", tool.tool),
                                        "state": "output-error",
                                        "toolCallId": tool.call_id,
                                        "input": input,
                                        "errorText": "[Tool execution was interrupted]"
                                    });
                                    if tool.metadata.as_ref().is_some_and(|m| {
                                        m.get("providerExecuted")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false)
                                    }) {
                                        part_value["providerExecuted"] =
                                            serde_json::Value::Bool(true);
                                    }
                                    if !different_model {
                                        if let Some(meta) = provider_meta(tool.metadata.as_ref()) {
                                            part_value["callProviderMetadata"] =
                                                serde_json::to_value(meta).unwrap();
                                        }
                                    }
                                    push_part_value(&mut assistant_message, part_value);
                                }
                                crate::v1::ToolState::Running(state) => {
                                    let input = state.input.clone();
                                    let mut part_value = serde_json::json!({
                                        "type": format!("tool-{}", tool.tool),
                                        "state": "output-error",
                                        "toolCallId": tool.call_id,
                                        "input": input,
                                        "errorText": "[Tool execution was interrupted]"
                                    });
                                    if tool.metadata.as_ref().is_some_and(|m| {
                                        m.get("providerExecuted")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false)
                                    }) {
                                        part_value["providerExecuted"] =
                                            serde_json::Value::Bool(true);
                                    }
                                    if !different_model {
                                        if let Some(meta) = provider_meta(tool.metadata.as_ref()) {
                                            part_value["callProviderMetadata"] =
                                                serde_json::to_value(meta).unwrap();
                                        }
                                    }
                                    push_part_value(&mut assistant_message, part_value);
                                }
                            }
                        }
                        Part::Reasoning(reasoning) => {
                            if different_model {
                                if !reasoning.text.trim().is_empty() {
                                    assistant_message.parts.push(UiPart::Text(UiTextPart {
                                        type_: "text".into(),
                                        text: reasoning.text.clone(),
                                        provider_metadata: None,
                                    }));
                                }
                                continue;
                            }
                            assistant_message
                                .parts
                                .push(UiPart::Reasoning(UiReasoningPart {
                                    type_: "reasoning".into(),
                                    text: reasoning.text.clone(),
                                    provider_metadata: reasoning.metadata.clone(),
                                }));
                        }
                        _ => {}
                    }
                }
                if !assistant_message.parts.is_empty() {
                    result.push(assistant_message);
                    if !media.is_empty() {
                        let mut media_msg = UiMessage {
                            id: crate::schema::create_message(None),
                            role: "user".to_string(),
                            parts: Vec::new(),
                        };
                        media_msg.parts.push(UiPart::Text(UiTextPart {
                            type_: "text".into(),
                            text: SYNTHETIC_ATTACHMENT_PROMPT.to_string(),
                            provider_metadata: None,
                        }));
                        for attachment in media {
                            media_msg.parts.push(UiPart::File(attachment));
                        }
                        result.push(media_msg);
                    }
                }
            }
        }
    }
    let _ = tool_names;
    result
}

fn push_part_value(message: &mut UiMessage, value: serde_json::Value) {
    let part: UiPart = serde_json::from_value(value).unwrap_or(UiPart::Text(UiTextPart {
        type_: "text".into(),
        text: String::new(),
        provider_metadata: None,
    }));
    message.parts.push(part);
}

#[derive(Debug, Clone, Default)]
pub struct ToModelOptions {
    pub strip_media: bool,
    pub tool_output_max_chars: Option<usize>,
}

/// `toModelOutput` result type — the `ToolResultPartResult` union.
#[derive(Debug, Clone)]
pub enum ToolResultResult {
    Text { value: String },
    Json { value: serde_json::Value },
    Content { value: Vec<serde_json::Value> },
    Error { message: String },
}

/// `toModelOutput` from reference `message-v2.ts` — produces the
/// `ToolResultPartResult` for a completed tool output.
pub fn to_model_output(output: &serde_json::Value) -> ToolResultResult {
    if let Some(text) = output.as_str() {
        return ToolResultResult::Text {
            value: text.to_string(),
        };
    }
    if let Some(object) = output.as_object() {
        let text = object.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let attachments = object
            .get("attachments")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|attachment| {
                attachment
                    .get("url")
                    .and_then(|v| v.as_str())
                    .is_some_and(|url| url.starts_with("data:") && url.contains(','))
            })
            .collect::<Vec<_>>();
        let mut value: Vec<serde_json::Value> = Vec::new();
        if !text.is_empty() {
            value.push(serde_json::json!({ "type": "text", "text": text }));
        }
        for attachment in attachments {
            let url = attachment.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let comma = url.find(',').unwrap_or(url.len());
            let data = &url[comma + 1..];
            value.push(serde_json::json!({
                "type": "media",
                "mediaType": attachment.get("mime").and_then(|v| v.as_str()).unwrap_or(""),
                "data": data
            }));
        }
        return ToolResultResult::Content { value };
    }
    ToolResultResult::Json {
        value: output.clone(),
    }
}

/// From reference `message-v2.ts:filterCompacted` — reorders messages for
/// model consumption after compaction.
pub fn filter_compacted(msgs: &[WithParts]) -> Vec<WithParts> {
    let mut result: Vec<WithParts> = Vec::new();
    let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut retain: Option<String> = None;
    for msg in msgs {
        result.push(msg.clone());
        if let Some(retain_id) = &retain {
            if msg.info.id() == retain_id {
                break;
            }
            continue;
        }
        if msg.info.role() == "user" && completed.contains(msg.info.id()) {
            let Some(part) = msg
                .parts
                .iter()
                .find(|item| matches!(item, Part::Compaction(_)))
            else {
                continue;
            };
            let Part::Compaction(compaction) = part else {
                continue;
            };
            if compaction.tail_start_id.is_none() {
                break;
            }
            retain = compaction.tail_start_id.clone();
            if msg.info.id() == retain.as_deref().unwrap_or("") {
                break;
            }
            continue;
        }
        if msg.info.role() == "user"
            && completed.contains(msg.info.id())
            && msg
                .parts
                .iter()
                .any(|part| matches!(part, Part::Compaction(_)))
        {
            break;
        }
        if let Info::Assistant(assistant) = &msg.info {
            if assistant.summary.unwrap_or(false)
                && assistant.finish.is_some()
                && assistant.error.is_none()
            {
                if let Some(parent_id) = Some(&assistant.parent_id) {
                    completed.insert(parent_id.clone());
                }
            }
        }
    }
    result.reverse();
    let compaction_index = result
        .iter()
        .rposition(|msg| {
            msg.info.role() == "user"
                && msg
                    .parts
                    .iter()
                    .any(|item| matches!(item, Part::Compaction(c) if c.tail_start_id.is_some()))
        })
        .unwrap_or(result.len());
    let compaction = result.get(compaction_index).cloned();
    let part = compaction.as_ref().and_then(|c| {
        c.parts
            .iter()
            .find(|item| matches!(item, Part::Compaction(p) if p.tail_start_id.is_some()))
    });
    let summary_index = match &compaction {
        Some(compaction) => result
            .iter()
            .position(|msg| {
                matches!(&msg.info, Info::Assistant(a) if a.summary.unwrap_or(false) && a.parent_id == compaction.info.id())
            })
            .map(|index| if index > compaction_index { index } else { usize::MAX })
            .unwrap_or(usize::MAX),
        None => usize::MAX,
    };
    let tail_index = part
        .and_then(|p| match p {
            Part::Compaction(c) => c.tail_start_id.as_ref(),
            _ => None,
        })
        .and_then(|tail| result.iter().position(|msg| msg.info.id() == tail))
        .unwrap_or(usize::MAX);
    if tail_index < result.len()
        && tail_index < compaction_index
        && summary_index < result.len()
        && summary_index > compaction_index
    {
        let mut next: Vec<WithParts> = Vec::new();
        next.extend(result[compaction_index..=summary_index].iter().cloned());
        next.extend(result[tail_index..compaction_index].iter().cloned());
        next.extend(result[summary_index + 1..].iter().cloned());
        return next;
    }
    result
}

#[derive(Debug, Clone)]
pub struct Latest {
    pub user: Option<crate::v1::User>,
    pub assistant: Option<crate::v1::Assistant>,
    pub finished: Option<crate::v1::Assistant>,
    pub tasks: Vec<(Part, String)>,
}

/// From reference `message-v2.ts:latest`.
pub fn latest(msgs: &[WithParts]) -> Latest {
    let mut user: Option<crate::v1::User> = None;
    let mut assistant: Option<crate::v1::Assistant> = None;
    let mut finished: Option<crate::v1::Assistant> = None;
    for msg in msgs {
        match &msg.info {
            Info::User(u) => {
                if user.as_ref().is_none_or(|current| u.id > current.id) {
                    user = Some(u.clone());
                }
            }
            Info::Assistant(a) => {
                if assistant.as_ref().is_none_or(|current| a.id > current.id) {
                    assistant = Some(a.clone());
                }
                if a.finish.is_some() && finished.as_ref().is_none_or(|current| a.id > current.id) {
                    finished = Some(a.clone());
                }
            }
        }
    }
    let mut tasks: Vec<(Part, String)> = Vec::new();
    for m in msgs {
        if let Some(finished) = &finished {
            if m.info.id() <= finished.id.as_str() {
                continue;
            }
        }
        for part in &m.parts {
            if matches!(part, Part::Compaction(_) | Part::Subtask(_)) {
                tasks.push((part.clone(), m.info.id().to_string()));
            }
        }
    }
    Latest {
        user,
        assistant,
        finished,
        tasks,
    }
}

/// Runtime error shapes recognized by `fromError` before provider-specific
/// classification.
#[derive(Debug, Clone)]
pub enum StreamError {
    /// DOMException("Aborted", "AbortError")
    Aborted(String),
    /// OutputLengthError
    OutputLength,
    /// LoadAPIKeyError
    LoadApiKey(String),
    /// Bun SystemError ECONNRESET
    EconnReset {
        code: String,
        syscall: String,
        message: String,
    },
    /// Fetch decompression failure (ZlibError)
    Zlib { code: String, message: String },
    /// ProviderError.HeaderTimeoutError
    HeaderTimeout { name: String, ms: u64 },
    /// ProviderError.ResponseStreamError
    ResponseStream { name: String },
    /// Classified API call error.
    ApiCall(ApiCallError),
    /// Plain Error
    Other(String),
    /// Non-error value serialized as JSON.
    Raw(serde_json::Value),
}

#[derive(Debug, Clone)]
pub enum ApiCallError {
    ContextOverflow {
        message: String,
        response_body: Option<String>,
    },
    Api {
        message: String,
        status_code: Option<u64>,
        is_retryable: bool,
        response_headers: Option<crate::JsonMap>,
        response_body: Option<String>,
        metadata: Option<crate::JsonMap>,
    },
}

#[derive(Debug, Clone)]
pub struct FromErrorCtx {
    pub provider_id: String,
    pub aborted: bool,
}

/// From reference `message-v2.ts:fromError`.
pub fn from_error(e: &StreamError, ctx: &FromErrorCtx) -> V1Error {
    match e {
        StreamError::Aborted(message) => V1Error::AbortedError {
            message: message.clone(),
        },
        StreamError::OutputLength => V1Error::OutputLengthError,
        StreamError::LoadApiKey(message) => V1Error::AuthError {
            provider_id: ctx.provider_id.clone(),
            message: message.clone(),
        },
        StreamError::EconnReset {
            code,
            syscall,
            message,
        } => V1Error::ApiError {
            message: "Connection reset by server".to_string(),
            status_code: None,
            is_retryable: true,
            response_headers: None,
            response_body: None,
            metadata: Some(crate::JsonMap::from_iter([
                ("code".to_string(), serde_json::Value::String(code.clone())),
                (
                    "syscall".to_string(),
                    serde_json::Value::String(syscall.clone()),
                ),
                (
                    "message".to_string(),
                    serde_json::Value::String(message.clone()),
                ),
            ])),
        },
        StreamError::Zlib { code, message } => {
            if ctx.aborted {
                V1Error::AbortedError {
                    message: message.clone(),
                }
            } else {
                V1Error::ApiError {
                    message: "Response decompression failed".to_string(),
                    status_code: None,
                    is_retryable: true,
                    response_headers: None,
                    response_body: None,
                    metadata: Some(crate::JsonMap::from_iter([
                        ("code".to_string(), serde_json::Value::String(code.clone())),
                        (
                            "message".to_string(),
                            serde_json::Value::String(message.clone()),
                        ),
                    ])),
                }
            }
        }
        StreamError::HeaderTimeout { name, ms } => V1Error::ApiError {
            message: name.clone(),
            status_code: None,
            is_retryable: true,
            response_headers: None,
            response_body: None,
            metadata: Some(crate::JsonMap::from_iter([
                ("code".to_string(), serde_json::Value::String(name.clone())),
                (
                    "timeoutMs".to_string(),
                    serde_json::Value::String(ms.to_string()),
                ),
            ])),
        },
        StreamError::ResponseStream { name } => V1Error::ApiError {
            message: name.clone(),
            status_code: None,
            is_retryable: true,
            response_headers: None,
            response_body: None,
            metadata: Some(crate::JsonMap::from_iter([(
                "code".to_string(),
                serde_json::Value::String(name.clone()),
            )])),
        },
        StreamError::ApiCall(api_call) => match api_call {
            ApiCallError::ContextOverflow {
                message,
                response_body,
            } => V1Error::ContextOverflowError {
                message: message.clone(),
                response_body: response_body.clone(),
            },
            ApiCallError::Api {
                message,
                status_code,
                is_retryable,
                response_headers,
                response_body,
                metadata,
            } => V1Error::ApiError {
                message: message.clone(),
                status_code: *status_code,
                is_retryable: *is_retryable,
                response_headers: response_headers.clone(),
                response_body: response_body.clone(),
                metadata: metadata.clone(),
            },
        },
        StreamError::Other(message) => V1Error::UnknownError {
            message: message.clone(),
            r#ref: None,
        },
        StreamError::Raw(value) => V1Error::UnknownError {
            message: value.to_string(),
            r#ref: None,
        },
    }
}

/// Final model message produced by `convert_to_model_messages` — mirrors the
/// AI SDK `ModelMessage` (`{ role, content }`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMessage {
    pub role: String,
    pub content: Vec<ModelContent>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ModelContent {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<crate::JsonMap>,
    },
    File {
        url: String,
        media_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<crate::JsonMap>,
    },
    #[serde(rename = "step-start")]
    StepStart,
    #[serde(rename = "tool-result")]
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        result: ToolResultResult,
    },
}

impl serde::Serialize for ToolResultResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = match self {
            ToolResultResult::Text { value } => {
                serde_json::json!({ "type": "text", "value": value })
            }
            ToolResultResult::Json { value } => {
                serde_json::json!({ "type": "json", "value": value })
            }
            ToolResultResult::Content { value } => {
                serde_json::json!({ "type": "content", "value": value })
            }
            ToolResultResult::Error { message } => {
                serde_json::json!({ "type": "error", "message": message })
            }
        };
        value.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ToolResultResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let type_ = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let inner = value
            .get("value")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        Ok(match type_ {
            "text" => ToolResultResult::Text {
                value: inner.as_str().unwrap_or_default().to_string(),
            },
            "content" => ToolResultResult::Content {
                value: inner.as_array().cloned().unwrap_or_default(),
            },
            "error" => ToolResultResult::Error {
                message: value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
            },
            _ => ToolResultResult::Json { value: inner },
        })
    }
}

/// From reference `message-v2.ts` — the `convertToModelMessages` lowering of
/// the UIMessage layer. Messages whose only parts are step-starts are dropped.
pub fn convert_to_model_messages(ui: Vec<UiMessage>) -> Vec<ModelMessage> {
    ui.into_iter()
        .filter(|message| {
            message
                .parts
                .iter()
                .any(|part| !matches!(part, UiPart::StepStart(_)))
        })
        .map(|message| ModelMessage {
            role: message.role,
            content: message.parts.into_iter().map(convert_part).collect(),
        })
        .collect()
}

fn convert_part(part: UiPart) -> ModelContent {
    match part {
        UiPart::Text(text) => ModelContent::Text {
            text: text.text,
            provider_metadata: text.provider_metadata,
        },
        UiPart::File(file) => ModelContent::File {
            url: file.url,
            media_type: file.media_type,
            filename: file.filename,
        },
        UiPart::Reasoning(reasoning) => ModelContent::Reasoning {
            text: reasoning.text,
            provider_metadata: reasoning.provider_metadata,
        },
        UiPart::StepStart(_) => ModelContent::StepStart,
        UiPart::Tool(tool) => {
            let tool_name = tool.type_.strip_prefix("tool-").unwrap_or("").to_string();
            let result = if tool.state == "output-available" {
                let output = tool
                    .output
                    .unwrap_or_else(|| serde_json::Value::String(String::new()));
                to_model_output(&output)
            } else {
                ToolResultResult::Error {
                    message: tool.error_text.unwrap_or_default(),
                }
            };
            ModelContent::ToolResult {
                tool_call_id: tool.tool_call_id,
                tool_name,
                result,
            }
        }
    }
}

/// Convenience: `toModelMessages` — full UIMessage → model message pipeline.
pub fn to_model_messages_final(
    input: &[WithParts],
    model: &ProviderModel,
    options: &ToModelOptions,
) -> Vec<ModelMessage> {
    convert_to_model_messages(to_model_messages(input, model, options))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JsonMap;

    #[test]
    fn cursor_round_trips() {
        let c = Cursor {
            id: "msg_abc".into(),
            time: 1234.5,
        };
        let encoded = cursor::encode(&c);
        assert!(!encoded.contains('='));
        let decoded = cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, c);
    }

    #[test]
    fn truncate_tool_output_notes_omitted_chars() {
        let text = "x".repeat(100);
        let truncated = truncate_tool_output(&text, Some(10));
        assert!(truncated.contains("[Tool output truncated for compaction: omitted 90 chars]"));
    }

    #[test]
    fn is_media_matches_images_and_pdf() {
        assert!(is_media("image/png"));
        assert!(is_media("application/pdf"));
        assert!(!is_media("text/plain"));
    }

    #[test]
    fn to_model_output_string() {
        let output = serde_json::json!("hello");
        match to_model_output(&output) {
            ToolResultResult::Text { value } => assert_eq!(value, "hello"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn from_error_abort() {
        let err = from_error(
            &StreamError::Aborted("Aborted".into()),
            &FromErrorCtx {
                provider_id: "openai".into(),
                aborted: false,
            },
        );
        assert!(matches!(err, V1Error::AbortedError { .. }));
    }

    #[test]
    fn filter_compacted_passthrough_when_no_compaction() {
        let user_part = Part::Text(crate::v1::TextPart {
            base: crate::v1::PartBase {
                id: "p".into(),
                session_id: "s".into(),
                message_id: "m".into(),
            },
            type_: "text".into(),
            text: "hi".into(),
            synthetic: None,
            ignored: None,
            time: None,
            metadata: None,
        });
        let msg = WithParts {
            info: Info::User(crate::v1::User {
                id: "m".into(),
                session_id: "s".into(),
                role: "user".into(),
                time: crate::v1::UserTime { created: 1 },
                format: None,
                summary: None,
                agent: "primary".into(),
                model: crate::v1::UserModel {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    variant: None,
                },
                system: None,
                tools: None,
            }),
            parts: vec![user_part],
        };
        let result = filter_compacted(std::slice::from_ref(&msg));
        assert_eq!(result.len(), 1);
        let _ = JsonMap::new();
    }

    #[test]
    fn latest_finds_newest_by_id() {
        let user1 = crate::v1::User {
            id: "msg1".into(),
            session_id: "s".into(),
            role: "user".into(),
            time: crate::v1::UserTime { created: 1 },
            format: None,
            summary: None,
            agent: "primary".into(),
            model: crate::v1::UserModel {
                provider_id: "p".into(),
                model_id: "m".into(),
                variant: None,
            },
            system: None,
            tools: None,
        };
        let user2 = crate::v1::User {
            id: "msg2".into(),
            ..user1.clone()
        };
        let msgs = vec![
            WithParts {
                info: Info::User(user1),
                parts: vec![],
            },
            WithParts {
                info: Info::User(user2),
                parts: vec![],
            },
        ];
        let latest = latest(&msgs);
        assert_eq!(latest.user.unwrap().id, "msg2");
    }
}
