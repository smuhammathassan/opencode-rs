//! OpenAI Responses protocol.
//! From reference/packages/llm/src/protocols/openai-responses.ts

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::utils::lifecycle;
use super::utils::openai_options as OpenAIOptions;
use super::utils::tool_schema::ToolSchemaProjection;
use super::utils::tool_stream::{self, ToolStream};
use crate::provider_error::is_context_overflow;
use crate::route::protocol::ProtocolStream;
use crate::route::Protocol;
use crate::schema::messages::{
    ContentPart, MediaData, MediaPart, ReasoningPart, TextPart, ToolCallPart, ToolContent,
    ToolDefinition, ToolResultPart,
};
use crate::schema::{FinishReason, LlmError, LlmEvent, LlmRequest, ToolChoiceType, Usage};
use crate::shared;

pub const ADAPTER: &str = "openai-responses";
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const PATH: &str = "/responses";

// =============================================================================
// Streaming Event Schema
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InputTokensDetails {
    #[serde(rename = "cached_tokens")]
    cached_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OutputTokensDetails {
    #[serde(rename = "reasoning_tokens")]
    reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OpenAIResponsesUsage {
    #[serde(rename = "input_tokens")]
    input_tokens: Option<i64>,
    #[serde(rename = "input_tokens_details")]
    input_tokens_details: Option<Option<InputTokensDetails>>,
    #[serde(rename = "output_tokens")]
    output_tokens: Option<i64>,
    #[serde(rename = "output_tokens_details")]
    output_tokens_details: Option<Option<OutputTokensDetails>>,
    #[serde(rename = "total_tokens")]
    total_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OpenAIResponsesStreamItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    id: Option<String>,
    #[serde(rename = "call_id")]
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
    status: Option<String>,
    action: Option<Value>,
    queries: Option<Value>,
    results: Option<Value>,
    code: Option<String>,
    #[serde(rename = "container_id")]
    container_id: Option<String>,
    outputs: Option<Value>,
    #[serde(rename = "server_label")]
    server_label: Option<String>,
    output: Option<Value>,
    error: Option<Value>,
    #[serde(rename = "encrypted_content")]
    encrypted_content: Option<Option<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct IncompleteDetails {
    reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
struct OpenAIResponsesErrorPayload {
    code: Option<Option<String>>,
    message: Option<Option<String>>,
    param: Option<Option<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct OpenAIResponsesResponse {
    id: Option<String>,
    #[serde(rename = "service_tier")]
    service_tier: Option<Option<String>>,
    #[serde(rename = "incomplete_details")]
    incomplete_details: Option<Option<IncompleteDetails>>,
    usage: Option<Option<OpenAIResponsesUsage>>,
    error: Option<Option<OpenAIResponsesErrorPayload>>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct OpenAIResponsesEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<String>,
    #[serde(rename = "item_id")]
    item_id: Option<String>,
    #[serde(rename = "summary_index")]
    summary_index: Option<i64>,
    item: Option<OpenAIResponsesStreamItem>,
    response: Option<OpenAIResponsesResponse>,
    code: Option<String>,
    message: Option<String>,
    param: Option<String>,
}

// =============================================================================
// Parser State
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ReasoningSummaryStatus {
    Active,
    CanConclude,
    Concluded,
}

#[derive(Debug, Clone)]
pub struct ReasoningStreamItem {
    encrypted_content: Option<Option<String>>,
    summary_parts: BTreeMap<i64, ReasoningSummaryStatus>,
}

#[derive(Clone)]
pub struct ParserState {
    pub tools: tool_stream::State<String>,
    pub has_function_call: bool,
    pub lifecycle: lifecycle::State,
    pub reasoning_items: BTreeMap<String, ReasoningStreamItem>,
    pub store: Option<bool>,
}

// =============================================================================
// Request Lowering
// =============================================================================

fn lower_tool(tool: &ToolDefinition, input_schema: &Value) -> Value {
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String("function".to_string()));
    obj.insert("name".to_string(), Value::String(tool.name.clone()));
    obj.insert(
        "description".to_string(),
        Value::String(tool.description.clone()),
    );
    obj.insert(
        "parameters".to_string(),
        ToolSchemaProjection::open_ai(input_schema),
    );
    obj.insert("strict".to_string(), Value::Bool(false));
    Value::Object(obj)
}

fn lower_tool_choice(tool_choice: &crate::schema::ToolChoice) -> Result<Value, LlmError> {
    match tool_choice.kind {
        ToolChoiceType::Auto => Ok(Value::String("auto".to_string())),
        ToolChoiceType::None => Ok(Value::String("none".to_string())),
        ToolChoiceType::Required => Ok(Value::String("required".to_string())),
        ToolChoiceType::Tool => {
            let Some(name) = &tool_choice.name else {
                return Err(shared::invalid_request(
                    "OpenAI Responses tool choice requires a tool name",
                ));
            };
            let mut obj = Map::new();
            obj.insert("type".to_string(), Value::String("function".to_string()));
            obj.insert("name".to_string(), Value::String(name.clone()));
            Ok(Value::Object(obj))
        }
    }
}

fn lower_tool_call(part: &ToolCallPart) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "type".to_string(),
        Value::String("function_call".to_string()),
    );
    obj.insert("call_id".to_string(), Value::String(part.id.clone()));
    obj.insert("name".to_string(), Value::String(part.name.clone()));
    obj.insert(
        "arguments".to_string(),
        Value::String(shared::encode_json(&part.input)),
    );
    Value::Object(obj)
}

fn lower_reasoning(part: &ReasoningPart) -> Option<ReasoningInput> {
    let openai = part
        .provider_metadata
        .as_ref()
        .and_then(|m| m.get("openai"));
    let openai = openai?;
    let item_id = openai.get("itemId").and_then(Value::as_str)?;
    if item_id.is_empty() {
        return None;
    }
    let encrypted_content = match openai.get("reasoningEncryptedContent") {
        Some(Value::String(s)) => Some(Some(s.clone())),
        Some(Value::Null) => Some(None),
        _ => None,
    };
    let summary = if part.text.is_empty() {
        Vec::new()
    } else {
        vec![Value::Object(Map::from_iter([
            (
                "type".to_string(),
                Value::String("summary_text".to_string()),
            ),
            ("text".to_string(), Value::String(part.text.clone())),
        ]))]
    };
    Some(ReasoningInput {
        id: item_id.to_string(),
        summary,
        encrypted_content,
    })
}

struct ReasoningInput {
    id: String,
    summary: Vec<Value>,
    encrypted_content: Option<Option<String>>,
}

fn hosted_tool_item_id(part: &ToolResultPart) -> Option<String> {
    let openai = part
        .provider_metadata
        .as_ref()
        .and_then(|m| m.get("openai"))?;
    let item_id = openai.get("itemId").and_then(Value::as_str)?;
    if item_id.is_empty() {
        None
    } else {
        Some(item_id.to_string())
    }
}

fn lower_user_content(part: &ContentPart) -> Result<Value, LlmError> {
    match part {
        ContentPart::Text { text, .. } => Ok(Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("input_text".to_string())),
            ("text".to_string(), Value::String(text.clone())),
        ]))),
        ContentPart::Media { .. } => {
            let media = media_part(part);
            let supported: std::collections::HashSet<String> =
                shared::IMAGE_MIMES.iter().map(|s| s.to_string()).collect();
            let media = shared::validate_media("OpenAI Responses", &media, &supported)?;
            Ok(Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("input_image".to_string())),
                ("image_url".to_string(), Value::String(media.data_url)),
            ])))
        }
        _ => Err(shared::unsupported(
            "OpenAI Responses",
            "user",
            &["text", "media"],
        )),
    }
}

fn lower_tool_result_content_item(item: &ToolContent) -> Result<Value, LlmError> {
    match item {
        ToolContent::Text { text } => Ok(Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("input_text".to_string())),
            ("text".to_string(), Value::String(text.clone())),
        ]))),
        ToolContent::File { .. } => {
            let supported: std::collections::HashSet<String> =
                shared::IMAGE_MIMES.iter().map(|s| s.to_string()).collect();
            let media =
                shared::validate_tool_file("OpenAI Responses", &tool_file(item), &supported)?;
            Ok(Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("input_image".to_string())),
                ("image_url".to_string(), Value::String(media.data_url)),
            ])))
        }
    }
}

fn lower_tool_result_output(part: &ToolResultPart) -> Result<Value, LlmError> {
    match &part.result {
        crate::schema::ToolResultValue::Content { value } => {
            let content: Vec<Value> = value
                .iter()
                .map(lower_tool_result_content_item)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(content))
        }
        _ => Ok(Value::String(shared::tool_result_text(part))),
    }
}

fn lower_messages(request: &LlmRequest) -> Result<Vec<Value>, LlmError> {
    let store = OpenAIOptions::store(request);
    let mut input: Vec<Value> = Vec::new();
    if !request.system.is_empty() {
        input.push(Value::Object(Map::from_iter([
            ("role".to_string(), Value::String("system".to_string())),
            (
                "content".to_string(),
                Value::String(shared::system_part_text(&request.system)),
            ),
        ])));
    }

    for message in &request.messages {
        if message.role == crate::schema::MessageRole::System {
            let part = shared::wrapped_system_update("OpenAI Responses", message)?;
            let previous = input.last().cloned();
            if let Some(Value::Object(prev)) = previous {
                if prev.get("role").and_then(Value::as_str) == Some("user") {
                    let mut next = prev.clone();
                    if let Some(Value::Array(content)) = next.get_mut("content") {
                        content.push(Value::Object(Map::from_iter([
                            ("type".to_string(), Value::String("input_text".to_string())),
                            ("text".to_string(), Value::String(part.text.clone())),
                        ])));
                    }
                    *input.last_mut().unwrap() = Value::Object(next);
                    continue;
                }
            }
            input.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                (
                    "content".to_string(),
                    Value::Array(vec![Value::Object(Map::from_iter([
                        ("type".to_string(), Value::String("input_text".to_string())),
                        ("text".to_string(), Value::String(part.text.clone())),
                    ]))]),
                ),
            ])));
            continue;
        }

        if message.role == crate::schema::MessageRole::User {
            let content = message
                .content
                .iter()
                .map(lower_user_content)
                .collect::<Result<Vec<_>, _>>()?;
            input.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                ("content".to_string(), Value::Array(content)),
            ])));
            continue;
        }

        if message.role == crate::schema::MessageRole::Assistant {
            let mut content: Vec<TextPart> = Vec::new();
            let mut reasoning_items: BTreeMap<String, ReasoningReplay> = BTreeMap::new();
            let mut reasoning_references: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut hosted_tool_references: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let flush_text = |input: &mut Vec<Value>, content: &mut Vec<TextPart>| {
                if content.is_empty() {
                    return;
                }
                input.push(Value::Object(Map::from_iter([
                    ("role".to_string(), Value::String("assistant".to_string())),
                    (
                        "content".to_string(),
                        Value::Array(
                            content
                                .iter()
                                .map(|part| {
                                    Value::Object(Map::from_iter([
                                        (
                                            "type".to_string(),
                                            Value::String("output_text".to_string()),
                                        ),
                                        ("text".to_string(), Value::String(part.text.clone())),
                                    ]))
                                })
                                .collect(),
                        ),
                    ),
                ])));
                content.clear();
            };

            for part in &message.content {
                match part {
                    ContentPart::Text { text, .. } => {
                        content.push(TextPart::make(text));
                    }
                    ContentPart::Reasoning {
                        text,
                        encrypted,
                        provider_metadata,
                        ..
                    } => {
                        flush_text(&mut input, &mut content);
                        let reasoning_part = ReasoningPart {
                            part_type: "reasoning".to_string(),
                            text: text.clone(),
                            encrypted: encrypted.clone(),
                            metadata: None,
                            provider_metadata: provider_metadata.clone(),
                        };
                        let Some(reasoning) = lower_reasoning(&reasoning_part) else {
                            continue;
                        };
                        if store != Some(false) {
                            if !reasoning_references.contains(&reasoning.id) {
                                input.push(Value::Object(Map::from_iter([
                                    (
                                        "type".to_string(),
                                        Value::String("item_reference".to_string()),
                                    ),
                                    ("id".to_string(), Value::String(reasoning.id.clone())),
                                ])));
                            }
                            reasoning_references.insert(reasoning.id);
                            continue;
                        }
                        let existing = reasoning_items.get_mut(&reasoning.id);
                        if let Some(existing) = existing {
                            existing.summary.extend(reasoning.summary.iter().cloned());
                            if let Some(Some(encrypted)) = reasoning.encrypted_content.clone() {
                                existing.encrypted_content = Some(Some(encrypted));
                            }
                            continue;
                        }
                        reasoning_items.insert(
                            reasoning.id.clone(),
                            ReasoningReplay {
                                summary: reasoning.summary.clone(),
                                encrypted_content: reasoning.encrypted_content.clone(),
                            },
                        );
                        let mut replay = Map::new();
                        replay.insert("type".to_string(), Value::String("reasoning".to_string()));
                        replay.insert("summary".to_string(), Value::Array(reasoning.summary));
                        crate::jset_opt!(replay, "encrypted_content", reasoning.encrypted_content);
                        input.push(Value::Object(replay));
                        continue;
                    }
                    ContentPart::ToolCall {
                        provider_executed, ..
                    } => {
                        flush_text(&mut input, &mut content);
                        if *provider_executed == Some(true) {
                            continue;
                        }
                        input.push(lower_tool_call(&tool_call_part(part)));
                        continue;
                    }
                    ContentPart::ToolResult {
                        provider_executed, ..
                    } if *provider_executed == Some(true) => {
                        flush_text(&mut input, &mut content);
                        let part = tool_result_part(part);
                        let item_id = hosted_tool_item_id(&part);
                        if store != Some(false) {
                            if let Some(item_id) = &item_id {
                                if !hosted_tool_references.contains(item_id) {
                                    input.push(Value::Object(Map::from_iter([
                                        (
                                            "type".to_string(),
                                            Value::String("item_reference".to_string()),
                                        ),
                                        ("id".to_string(), Value::String(item_id.clone())),
                                    ])));
                                }
                            }
                        }
                        if let Some(item_id) = item_id {
                            hosted_tool_references.insert(item_id);
                        }
                        continue;
                    }
                    _ => {
                        return Err(shared::unsupported(
                            "OpenAI Responses",
                            "assistant",
                            &["text", "reasoning", "tool-call", "tool-result"],
                        ));
                    }
                }
            }
            flush_text(&mut input, &mut content);
            continue;
        }

        for part in &message.content {
            let ContentPart::ToolResult { .. } = part else {
                return Err(shared::unsupported(
                    "OpenAI Responses",
                    "tool",
                    &["tool-result"],
                ));
            };
            let part = tool_result_part(part);
            let mut obj = Map::new();
            obj.insert(
                "type".to_string(),
                Value::String("function_call_output".to_string()),
            );
            obj.insert("call_id".to_string(), Value::String(part.id.clone()));
            obj.insert("output".to_string(), lower_tool_result_output(&part)?);
            input.push(Value::Object(obj));
        }
    }

    if store == Some(false) {
        input.retain(|item| {
            if let Value::Object(obj) = item {
                if obj.get("type").and_then(Value::as_str) == Some("reasoning") {
                    return obj
                        .get("encrypted_content")
                        .map(|v| v.is_string())
                        .unwrap_or(false);
                }
            }
            true
        });
    }
    Ok(input)
}

struct ReasoningReplay {
    summary: Vec<Value>,
    encrypted_content: Option<Option<String>>,
}

fn lower_options(request: &LlmRequest) -> Result<Map<String, Value>, LlmError> {
    let store = OpenAIOptions::store(request);
    let prompt_cache_key = OpenAIOptions::prompt_cache_key(request);
    let effort = OpenAIOptions::reasoning_effort(request);
    if let Some(effort) = &effort {
        if !OpenAIOptions::is_reasoning_effort(effort) {
            return Err(shared::invalid_request(format!(
                "OpenAI Responses does not support reasoning effort {}",
                effort
            )));
        }
    }
    let summary = OpenAIOptions::reasoning_summary(request);
    let include = OpenAIOptions::include(request);
    let verbosity = OpenAIOptions::text_verbosity(request);
    let instructions = OpenAIOptions::instructions(request);
    let service_tier = OpenAIOptions::service_tier(request);

    let mut options = Map::new();
    crate::jset_opt!(options, "instructions", instructions);
    crate::jset_opt!(options, "store", store);
    crate::jset_opt!(options, "prompt_cache_key", prompt_cache_key);
    crate::jset_opt!(options, "include", include);
    if effort.is_some() || summary.is_some() {
        let mut reasoning = Map::new();
        crate::jset_opt!(reasoning, "effort", effort);
        crate::jset_opt!(reasoning, "summary", summary);
        options.insert("reasoning".to_string(), Value::Object(reasoning));
    }
    if let Some(verbosity) = verbosity {
        options.insert(
            "text".to_string(),
            Value::Object(Map::from_iter([(
                "verbosity".to_string(),
                Value::String(verbosity),
            )])),
        );
    }
    crate::jset_opt!(options, "service_tier", service_tier);
    Ok(options)
}

/// `OpenAIResponses.fromRequest`.
/// From reference/packages/llm/src/protocols/openai-responses.ts (`fromRequest`)
pub fn from_request(request: &LlmRequest) -> Result<Value, LlmError> {
    let generation = request.generation.clone();
    let options = lower_options(request)?;
    let tool_schema_compatibility = request
        .model
        .compatibility
        .as_ref()
        .and_then(|c| c.tool_schema);
    let mut body = Map::new();
    body.insert(
        "model".to_string(),
        Value::String(request.model.id.0.clone()),
    );
    body.insert("input".to_string(), Value::Array(lower_messages(request)?));
    if !request.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        lower_tool(
                            tool,
                            &ToolSchemaProjection::model_compatibility(
                                &tool.input_schema,
                                tool_schema_compatibility,
                            ),
                        )
                    })
                    .collect(),
            ),
        );
    }
    if let Some(tool_choice) = &request.tool_choice {
        body.insert("tool_choice".to_string(), lower_tool_choice(tool_choice)?);
    }
    body.insert("stream".to_string(), Value::Bool(true));
    crate::jset_opt!(
        body,
        "max_output_tokens",
        generation.as_ref().and_then(|g| g.max_tokens)
    );
    crate::jset_opt!(
        body,
        "temperature",
        generation
            .as_ref()
            .and_then(|g| g.temperature)
            .map(shared::json_number)
    );
    crate::jset_opt!(
        body,
        "top_p",
        generation
            .as_ref()
            .and_then(|g| g.top_p)
            .map(shared::json_number)
    );
    for (key, value) in options {
        body.insert(key, value);
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Stream Parsing
// =============================================================================

fn map_usage(usage: Option<OpenAIResponsesUsage>) -> Option<Usage> {
    let usage = usage?;
    let cached = usage
        .input_tokens_details
        .as_ref()
        .and_then(|d| d.as_ref())
        .and_then(|d| d.cached_tokens);
    let reasoning = usage
        .output_tokens_details
        .as_ref()
        .and_then(|d| d.as_ref())
        .and_then(|d| d.reasoning_tokens);
    let non_cached = shared::subtract_tokens(usage.input_tokens, cached);
    let raw = serde_json::to_value(&usage).unwrap_or(Value::Null);
    Some(Usage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cached,
        reasoning_tokens: reasoning,
        total_tokens: shared::total_tokens(
            usage.input_tokens,
            usage.output_tokens,
            usage.total_tokens,
        ),
        cache_write_input_tokens: None,
        provider_metadata: Some(crate::schema::ProviderMetadata::from_iter([(
            "openai".to_string(),
            raw.as_object().cloned().unwrap_or_default(),
        )])),
    })
}

fn map_finish_reason(event: &OpenAIResponsesEvent, has_function_call: bool) -> FinishReason {
    let reason = event
        .response
        .as_ref()
        .and_then(|r| r.incomplete_details.as_ref())
        .and_then(|v| v.as_ref())
        .and_then(|d| d.reason.clone());
    match reason.as_deref() {
        None | Some("") => {
            if has_function_call {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
        Some("max_output_tokens") => FinishReason::Length,
        Some("content_filter") => FinishReason::ContentFilter,
        _ => {
            if has_function_call {
                FinishReason::ToolCalls
            } else {
                FinishReason::Unknown
            }
        }
    }
}

fn openai_metadata(metadata: serde_json::Map<String, Value>) -> crate::schema::ProviderMetadata {
    crate::schema::ProviderMetadata::from_iter([("openai".to_string(), metadata)])
}

fn hosted_tool_events(item: &OpenAIResponsesStreamItem) -> Vec<LlmEvent> {
    let tool = HOSTED_TOOLS
        .iter()
        .find(|(kind, _, _)| Some(*kind) == item.item_type.as_deref());
    match tool {
        Some((_, name, input)) => {
            let id = item.id.clone().unwrap_or_default();
            let provider_metadata = openai_metadata(Map::from_iter([(
                "itemId".to_string(),
                Value::String(id.clone()),
            )]));
            vec![
                LlmEvent::ToolCall {
                    id: id.clone(),
                    name: name.to_string(),
                    input: input(item),
                    provider_executed: Some(true),
                    provider_metadata: Some(provider_metadata.clone()),
                },
                LlmEvent::ToolResult {
                    id,
                    name: name.to_string(),
                    result: hosted_tool_result(item),
                    output: None,
                    provider_executed: Some(true),
                    provider_metadata: Some(provider_metadata),
                },
            ]
        }
        None => vec![],
    }
}

fn hosted_tool_result(item: &OpenAIResponsesStreamItem) -> crate::schema::ToolResultValue {
    let is_error = item.error.is_some() && !item.error.is_none();
    if is_error {
        crate::schema::ToolResultValue::Error {
            value: item.error.clone().unwrap_or(Value::Null),
        }
    } else {
        crate::schema::ToolResultValue::Json {
            value: serde_json::to_value(item).unwrap_or(Value::Null),
        }
    }
}

const HOSTED_TOOLS: [(&str, &str, fn(&OpenAIResponsesStreamItem) -> Value); 8] = [
    ("web_search_call", "web_search", |item| {
        item.action.clone().unwrap_or(Value::Object(Map::new()))
    }),
    ("web_search_preview_call", "web_search_preview", |item| {
        item.action.clone().unwrap_or(Value::Object(Map::new()))
    }),
    ("file_search_call", "file_search", |item| {
        Value::Object(Map::from_iter([(
            "queries".to_string(),
            item.queries.clone().unwrap_or_else(|| Value::Array(vec![])),
        )]))
    }),
    ("code_interpreter_call", "code_interpreter", |item| {
        let mut obj = Map::new();
        crate::jset_opt!(obj, "code", item.code.clone());
        crate::jset_opt!(obj, "container_id", item.container_id.clone());
        Value::Object(obj)
    }),
    ("computer_use_call", "computer_use", |item| {
        item.action.clone().unwrap_or(Value::Object(Map::new()))
    }),
    ("image_generation_call", "image_generation", |_item| {
        Value::Object(Map::new())
    }),
    ("mcp_call", "mcp", |item| {
        let mut obj = Map::new();
        crate::jset_opt!(obj, "server_label", item.server_label.clone());
        crate::jset_opt!(obj, "name", item.name.clone());
        crate::jset_opt!(obj, "arguments", item.arguments.clone());
        Value::Object(obj)
    }),
    ("local_shell_call", "local_shell", |item| {
        item.action.clone().unwrap_or(Value::Object(Map::new()))
    }),
];

type StepResult = (ParserState, Vec<LlmEvent>);

fn on_output_text_delta(state: &ParserState, event: &OpenAIResponsesEvent) -> StepResult {
    let Some(delta) = &event.delta else {
        return (state.clone(), vec![]);
    };
    let mut events = Vec::new();
    let mut next = state.clone();
    next.lifecycle = lifecycle::text_delta(
        &state.lifecycle,
        &mut events,
        event.item_id.as_deref().unwrap_or("text-0"),
        delta,
    );
    (next, events)
}

fn on_reasoning_delta(state: &ParserState, event: &OpenAIResponsesEvent) -> StepResult {
    let Some(delta) = &event.delta else {
        return (state.clone(), vec![]);
    };
    let mut events = Vec::new();
    let item_id = event.item_id.as_deref().unwrap_or("reasoning-0");
    let id = if event.summary_index.is_some() || state.reasoning_items.contains_key(item_id) {
        format!("{}:{}", item_id, event.summary_index.unwrap_or(0))
    } else {
        item_id.to_string()
    };
    let mut next = state.clone();
    next.lifecycle = lifecycle::reasoning_delta(&state.lifecycle, &mut events, &id, delta, None);
    (next, events)
}

fn on_reasoning_done(state: &ParserState, _event: &OpenAIResponsesEvent) -> StepResult {
    (state.clone(), vec![])
}

fn reasoning_metadata(
    item_id: &str,
    encrypted: Option<Option<String>>,
) -> crate::schema::ProviderMetadata {
    let mut obj = Map::new();
    obj.insert("itemId".to_string(), Value::String(item_id.to_string()));
    obj.insert(
        "reasoningEncryptedContent".to_string(),
        encrypted
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    openai_metadata(obj)
}

fn on_output_item_added(state: &ParserState, event: &OpenAIResponsesEvent) -> StepResult {
    let item = event.item.as_ref();
    if let Some(item) = item {
        if is_reasoning_item(item) {
            let mut events = Vec::new();
            let id = item.id.clone().unwrap();
            let mut next = state.clone();
            next.lifecycle = lifecycle::reasoning_start(
                &state.lifecycle,
                &mut events,
                &format!("{}:0", id),
                Some(&reasoning_metadata(&id, item.encrypted_content.clone())),
            );
            next.reasoning_items.insert(
                id.clone(),
                ReasoningStreamItem {
                    encrypted_content: item.encrypted_content.clone(),
                    summary_parts: BTreeMap::from_iter([(0, ReasoningSummaryStatus::Active)]),
                },
            );
            return (next, events);
        }
        if item.item_type.as_deref() != Some("function_call") || item.id.is_none() {
            return (state.clone(), vec![]);
        }
        let id = item.id.clone().unwrap();
        let provider_metadata = openai_metadata(Map::from_iter([(
            "itemId".to_string(),
            Value::String(id.clone()),
        )]));
        let mut events = Vec::new();
        let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
        let mut next = state.clone();
        next.lifecycle = lifecycle;
        next.tools = ToolStream::start(
            &state.tools,
            id.clone(),
            tool_stream::PendingToolInput {
                id: item.call_id.clone().unwrap_or_else(|| id.clone()),
                name: item.name.clone().unwrap_or_default(),
                input: item.arguments.clone(),
                provider_executed: None,
                provider_metadata: Some(provider_metadata.clone()),
            },
        );
        events.push(LlmEvent::ToolInputStart {
            id: item.call_id.clone().unwrap_or_else(|| id.clone()),
            name: item.name.clone().unwrap_or_default(),
            provider_metadata: Some(provider_metadata),
        });
        return (next, events);
    }
    (state.clone(), vec![])
}

fn on_reasoning_summary_part_added(
    state: &ParserState,
    event: &OpenAIResponsesEvent,
) -> StepResult {
    let Some(item_id) = &event.item_id else {
        return (state.clone(), vec![]);
    };
    let Some(summary_index) = event.summary_index else {
        return (state.clone(), vec![]);
    };
    let item = state
        .reasoning_items
        .get(item_id)
        .cloned()
        .unwrap_or(ReasoningStreamItem {
            encrypted_content: None,
            summary_parts: BTreeMap::new(),
        });
    if summary_index == 0 {
        if state.reasoning_items.contains_key(item_id) {
            return (state.clone(), vec![]);
        }
        let mut events = Vec::new();
        let mut next = state.clone();
        next.lifecycle = lifecycle::reasoning_start(
            &state.lifecycle,
            &mut events,
            &format!("{}:0", item_id),
            Some(&openai_metadata(Map::from_iter([
                ("itemId".to_string(), Value::String(item_id.clone())),
                ("reasoningEncryptedContent".to_string(), Value::Null),
            ]))),
        );
        next.reasoning_items.insert(
            item_id.clone(),
            ReasoningStreamItem {
                encrypted_content: item.encrypted_content.clone(),
                summary_parts: BTreeMap::from_iter([(0, ReasoningSummaryStatus::Active)]),
            },
        );
        return (next, events);
    }

    let mut events = Vec::new();
    let mut lifecycle = state.lifecycle.clone();
    for (key, status) in &item.summary_parts {
        if *status == ReasoningSummaryStatus::CanConclude {
            lifecycle = lifecycle::reasoning_end(
                &lifecycle,
                &mut events,
                &format!("{}:{}", item_id, key),
                Some(&openai_metadata(Map::from_iter([(
                    "itemId".to_string(),
                    Value::String(item_id.clone()),
                )]))),
            );
        }
    }
    lifecycle = lifecycle::reasoning_start(
        &lifecycle,
        &mut events,
        &format!("{}:{}", item_id, summary_index),
        Some(&reasoning_metadata(item_id, item.encrypted_content.clone())),
    );
    let mut next = state.clone();
    next.lifecycle = lifecycle;
    let mut summary_parts = item.summary_parts.clone();
    for (_key, status) in summary_parts.iter_mut() {
        if *status == ReasoningSummaryStatus::CanConclude {
            *status = ReasoningSummaryStatus::Concluded;
        }
    }
    summary_parts.insert(summary_index, ReasoningSummaryStatus::Active);
    next.reasoning_items.insert(
        item_id.clone(),
        ReasoningStreamItem {
            encrypted_content: item.encrypted_content.clone(),
            summary_parts,
        },
    );
    (next, events)
}

fn on_reasoning_summary_part_done(state: &ParserState, event: &OpenAIResponsesEvent) -> StepResult {
    let Some(item_id) = &event.item_id else {
        return (state.clone(), vec![]);
    };
    let Some(summary_index) = event.summary_index else {
        return (state.clone(), vec![]);
    };
    let Some(item) = state.reasoning_items.get(item_id).cloned() else {
        return (state.clone(), vec![]);
    };
    let mut events = Vec::new();
    let mut next = state.clone();
    let status = if state.store != Some(false) {
        next.lifecycle = lifecycle::reasoning_end(
            &state.lifecycle,
            &mut events,
            &format!("{}:{}", item_id, summary_index),
            Some(&openai_metadata(Map::from_iter([(
                "itemId".to_string(),
                Value::String(item_id.clone()),
            )]))),
        );
        ReasoningSummaryStatus::Concluded
    } else {
        ReasoningSummaryStatus::CanConclude
    };
    let mut summary_parts = item.summary_parts.clone();
    summary_parts.insert(summary_index, status);
    next.reasoning_items.insert(
        item_id.clone(),
        ReasoningStreamItem {
            encrypted_content: item.encrypted_content.clone(),
            summary_parts,
        },
    );
    (next, events)
}

fn on_function_call_arguments_delta(
    state: &ParserState,
    event: &OpenAIResponsesEvent,
) -> Result<StepResult, LlmError> {
    let Some(item_id) = &event.item_id else {
        return Ok((state.clone(), vec![]));
    };
    let Some(delta) = &event.delta else {
        return Ok((state.clone(), vec![]));
    };
    let result = tool_stream::append_existing(
        ADAPTER,
        &state.tools,
        item_id,
        delta,
        "OpenAI Responses tool argument delta is missing its tool call",
    )?;
    let mut events = Vec::new();
    let lifecycle = if !result.events.is_empty() {
        lifecycle::step_start(&state.lifecycle, &mut events)
    } else {
        state.lifecycle.clone()
    };
    events.extend(result.events);
    let mut next = state.clone();
    next.lifecycle = lifecycle;
    next.tools = result.tools;
    Ok((next, events))
}

fn is_reasoning_item(item: &OpenAIResponsesStreamItem) -> bool {
    item.item_type.as_deref() == Some("reasoning")
        && item.id.as_deref().map(|id| !id.is_empty()).unwrap_or(false)
}

fn on_output_item_done(
    state: &ParserState,
    event: &OpenAIResponsesEvent,
) -> Result<StepResult, LlmError> {
    let Some(item) = &event.item else {
        return Ok((state.clone(), vec![]));
    };

    if item.item_type.as_deref() == Some("function_call") {
        let Some(id) = &item.id else {
            return Ok((state.clone(), vec![]));
        };
        if item.call_id.is_none() || item.name.is_none() {
            return Ok((state.clone(), vec![]));
        }
        let tools = if state.tools.contains_key(id) {
            state.tools.clone()
        } else {
            ToolStream::start(
                &state.tools,
                id.clone(),
                tool_stream::PendingToolInput {
                    id: item.call_id.clone().unwrap(),
                    name: item.name.clone().unwrap(),
                    input: None,
                    provider_executed: None,
                    provider_metadata: None,
                },
            )
        };
        let result = match &item.arguments {
            None => tool_stream::finish(ADAPTER, &tools, id)?,
            Some(arguments) => tool_stream::finish_with_input(ADAPTER, &tools, id, arguments)?,
        };
        let mut events = Vec::new();
        let result_events = result.events;
        let lifecycle = if !result_events.is_empty() {
            lifecycle::step_start(&state.lifecycle, &mut events)
        } else {
            state.lifecycle.clone()
        };
        events.extend(result_events);
        let mut next = state.clone();
        next.lifecycle = lifecycle;
        next.has_function_call = events
            .iter()
            .any(|e| matches!(e, LlmEvent::ToolCall { .. }))
            || state.has_function_call;
        next.tools = result.tools;
        return Ok((next, events));
    }

    if is_hosted_tool_item(item) {
        let mut events = Vec::new();
        let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
        events.extend(hosted_tool_events(item));
        let mut next = state.clone();
        next.lifecycle = lifecycle;
        return Ok((next, events));
    }

    if is_reasoning_item(item) {
        let mut events = Vec::new();
        let id = item.id.clone().unwrap();
        let provider_metadata = reasoning_metadata(&id, item.encrypted_content.clone());
        let reasoning_item = state.reasoning_items.get(&id).cloned();
        if let Some(reasoning_item) = reasoning_item {
            let mut lifecycle = state.lifecycle.clone();
            for (key, status) in &reasoning_item.summary_parts {
                if *status == ReasoningSummaryStatus::Active
                    || *status == ReasoningSummaryStatus::CanConclude
                {
                    lifecycle = lifecycle::reasoning_end(
                        &lifecycle,
                        &mut events,
                        &format!("{}:{}", id, key),
                        Some(&provider_metadata),
                    );
                }
            }
            let mut next = state.clone();
            next.lifecycle = lifecycle;
            next.reasoning_items.remove(&id);
            return Ok((next, events));
        }
        if !state.lifecycle.reasoning.contains(&id) {
            let mut next = state.clone();
            let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
            events.push(LlmEvent::ReasoningStart {
                id: id.clone(),
                provider_metadata: Some(provider_metadata.clone()),
            });
            events.push(LlmEvent::ReasoningEnd {
                id: id.clone(),
                provider_metadata: Some(provider_metadata),
            });
            next.lifecycle = lifecycle;
            return Ok((next, events));
        }
        let mut next = state.clone();
        next.lifecycle =
            lifecycle::reasoning_end(&state.lifecycle, &mut events, &id, Some(&provider_metadata));
        return Ok((next, events));
    }

    Ok((state.clone(), vec![]))
}

fn is_hosted_tool_item(item: &OpenAIResponsesStreamItem) -> bool {
    item.item_type
        .as_deref()
        .map(|kind| HOSTED_TOOLS.iter().any(|(k, _, _)| *k == kind))
        .unwrap_or(false)
        && item.id.as_deref().map(|id| !id.is_empty()).unwrap_or(false)
}

fn on_response_finish(state: &ParserState, event: &OpenAIResponsesEvent) -> StepResult {
    let mut events = Vec::new();
    let mut provider_metadata = None;
    if let Some(response) = &event.response {
        if response.id.is_some() || response.service_tier.is_some() {
            let mut obj = Map::new();
            crate::jset_opt!(obj, "responseId", response.id.clone());
            crate::jset_opt!(obj, "serviceTier", response.service_tier.clone().flatten());
            provider_metadata = Some(openai_metadata(obj));
        }
    }
    let lifecycle = lifecycle::finish(
        &state.lifecycle,
        &mut events,
        map_finish_reason(event, state.has_function_call),
        map_usage(
            event
                .response
                .as_ref()
                .and_then(|r| r.usage.as_ref())
                .and_then(|v| v.as_ref())
                .cloned(),
        )
        .as_ref(),
        provider_metadata.as_ref(),
    );
    let mut next = state.clone();
    next.lifecycle = lifecycle;
    (next, events)
}

fn provider_error_message(event: &OpenAIResponsesEvent, fallback: &str) -> String {
    let nested = event
        .response
        .as_ref()
        .and_then(|r| r.error.as_ref())
        .and_then(|v| v.as_ref());
    let message = event
        .message
        .clone()
        .or_else(|| {
            nested
                .as_ref()
                .and_then(|e| e.message.as_ref())
                .and_then(|v| v.as_ref())
                .cloned()
        })
        .or_else(|| {
            nested
                .as_ref()
                .and_then(|e| e.code.as_ref())
                .and_then(|v| v.as_ref())
                .cloned()
        })
        .unwrap_or_else(|| fallback.to_string());
    let code = event.code.clone().or_else(|| {
        nested
            .as_ref()
            .and_then(|e| e.code.as_ref())
            .and_then(|v| v.as_ref())
            .cloned()
    });
    match (message.as_str(), code.as_deref()) {
        (m, Some(c)) if !m.is_empty() => format!("{}: {}", c, m),
        (m, None) => m.to_string(),
        (_, Some(c)) => c.to_string(),
    }
}

fn provider_error(event: &OpenAIResponsesEvent, fallback: &str) -> LlmEvent {
    let code = event.code.clone().or_else(|| {
        event
            .response
            .as_ref()
            .and_then(|r| r.error.as_ref())
            .and_then(|v| v.as_ref())
            .and_then(|e| e.code.as_ref())
            .and_then(|v| v.as_ref())
            .cloned()
    });
    let message = provider_error_message(event, fallback);
    let classification =
        if code.as_deref() == Some("context_length_exceeded") || is_context_overflow(&message) {
            Some(crate::schema::ProviderFailureClassification::ContextOverflow)
        } else {
            None
        };
    LlmEvent::ProviderError {
        message,
        classification,
        retryable: None,
        provider_metadata: None,
    }
}

fn on_response_failed(state: &ParserState, event: &OpenAIResponsesEvent) -> StepResult {
    (
        state.clone(),
        vec![provider_error(event, "OpenAI Responses response failed")],
    )
}

fn on_error(state: &ParserState, event: &OpenAIResponsesEvent) -> StepResult {
    (
        state.clone(),
        vec![provider_error(event, "OpenAI Responses stream error")],
    )
}

fn step(state: &mut ParserState, event: &OpenAIResponsesEvent) -> Result<Vec<LlmEvent>, LlmError> {
    match event.event_type.as_str() {
        "response.output_text.delta" => {
            let (next, events) = on_output_text_delta(state, event);
            *state = next;
            Ok(events)
        }
        "response.reasoning_text.delta"
        | "response.reasoning_summary.delta"
        | "response.reasoning_summary_text.delta" => {
            let (next, events) = on_reasoning_delta(state, event);
            *state = next;
            Ok(events)
        }
        "response.reasoning_text.done"
        | "response.reasoning_summary.done"
        | "response.reasoning_summary_text.done" => {
            let (next, events) = on_reasoning_done(state, event);
            *state = next;
            Ok(events)
        }
        "response.reasoning_summary_part.added" => {
            let (next, events) = on_reasoning_summary_part_added(state, event);
            *state = next;
            Ok(events)
        }
        "response.reasoning_summary_part.done" => {
            let (next, events) = on_reasoning_summary_part_done(state, event);
            *state = next;
            Ok(events)
        }
        "response.output_item.added" => {
            let (next, events) = on_output_item_added(state, event);
            *state = next;
            Ok(events)
        }
        "response.function_call_arguments.delta" => {
            let (next, events) = on_function_call_arguments_delta(state, event)?;
            *state = next;
            Ok(events)
        }
        "response.output_item.done" => {
            let (next, events) = on_output_item_done(state, event)?;
            *state = next;
            Ok(events)
        }
        "response.completed" | "response.incomplete" => {
            let (next, events) = on_response_finish(state, event);
            *state = next;
            Ok(events)
        }
        "response.failed" => {
            let (next, events) = on_response_failed(state, event);
            *state = next;
            Ok(events)
        }
        "error" => {
            let (next, events) = on_error(state, event);
            *state = next;
            Ok(events)
        }
        _ => Ok(vec![]),
    }
}

// =============================================================================
// Protocol
// =============================================================================

const TERMINAL_TYPES: [&str; 3] = [
    "response.completed",
    "response.incomplete",
    "response.failed",
];

struct OpenAIResponsesStream;

impl ProtocolStream for OpenAIResponsesStream {
    fn initial(&self, request: &LlmRequest) -> Box<dyn Any + Send> {
        Box::new(ParserState {
            tools: ToolStream::empty(),
            has_function_call: false,
            lifecycle: lifecycle::initial(),
            reasoning_items: BTreeMap::new(),
            store: OpenAIOptions::store(request),
        })
    }

    fn step(
        &self,
        state: Box<dyn Any + Send>,
        event: &Value,
    ) -> Result<(Box<dyn Any + Send>, Vec<LlmEvent>), LlmError> {
        let mut state = *state
            .downcast::<ParserState>()
            .map_err(|_| shared::invalid_request("OpenAI Responses parser state mismatch"))?;
        let event: OpenAIResponsesEvent = serde_json::from_value(event.clone()).unwrap_or_default();
        let events = step(&mut state, &event)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, event: &Value) -> bool {
        event
            .get("type")
            .and_then(Value::as_str)
            .map(|kind| TERMINAL_TYPES.contains(&kind))
            .unwrap_or(false)
    }

    fn on_halt(&self, _state: Box<dyn Any + Send>) -> Vec<LlmEvent> {
        vec![]
    }
}

/// `OpenAIResponses.protocol`.
/// From reference/packages/llm/src/protocols/openai-responses.ts (`protocol`)
pub fn protocol() -> Protocol {
    Protocol::make(
        ADAPTER,
        Arc::new(|request| from_request(request)),
        Arc::new(OpenAIResponsesStream),
    )
}

/// `OpenAIResponses.route`.
/// From reference/packages/llm/src/protocols/openai-responses.ts (`route`)
pub fn route() -> crate::route::Route {
    crate::route::Route::make(crate::route::RouteMakeInput {
        id: ADAPTER.to_string(),
        provider: Some("openai".to_string()),
        protocol: protocol(),
        endpoint: crate::route::endpoint::path(
            PATH,
            crate::route::EndpointOptions {
                base_url: Some(DEFAULT_BASE_URL.to_string()),
                query: None,
            },
        ),
        auth: Some(crate::route::Auth::none()),
        framing: Some(crate::route::Framing::Sse),
        headers: None,
        defaults: Some(crate::route::RouteDefaultsInput {
            provider_options: Some(crate::schema::ProviderOptions::from_iter([(
                "openai".to_string(),
                std::collections::BTreeMap::from_iter([("store".to_string(), Value::Bool(false))]),
            )])),
            ..Default::default()
        }),
    })
}

// =============================================================================
// Part accessors
// =============================================================================

fn media_part(part: &ContentPart) -> MediaPart {
    match part {
        ContentPart::Media {
            media_type,
            data,
            filename,
            metadata,
        } => MediaPart {
            part_type: "media".to_string(),
            media_type: media_type.clone(),
            data: data.clone(),
            filename: filename.clone(),
            metadata: metadata.clone(),
        },
        _ => unreachable!(),
    }
}

fn tool_call_part(part: &ContentPart) -> ToolCallPart {
    match part {
        ContentPart::ToolCall {
            id,
            name,
            input,
            provider_executed,
            metadata,
            provider_metadata,
        } => ToolCallPart {
            part_type: "tool-call".to_string(),
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
            provider_executed: *provider_executed,
            metadata: metadata.clone(),
            provider_metadata: provider_metadata.clone(),
        },
        _ => unreachable!(),
    }
}

fn tool_result_part(part: &ContentPart) -> ToolResultPart {
    match part {
        ContentPart::ToolResult {
            id,
            name,
            result,
            provider_executed,
            cache,
            metadata,
            provider_metadata,
        } => ToolResultPart {
            part_type: "tool-result".to_string(),
            id: id.clone(),
            name: name.clone(),
            result: result.clone(),
            provider_executed: *provider_executed,
            cache: cache.clone(),
            metadata: metadata.clone(),
            provider_metadata: provider_metadata.clone(),
        },
        _ => unreachable!(),
    }
}

fn tool_file(item: &ToolContent) -> crate::schema::messages::ToolFileContent {
    match item {
        ToolContent::File { uri, mime, name } => crate::schema::messages::ToolFileContent {
            part_type: "file".to_string(),
            uri: uri.clone(),
            mime: mime.clone(),
            name: name.clone(),
        },
        _ => unreachable!(),
    }
}

#[allow(unused)]
fn _media_data_marker(_: &MediaData) {}
