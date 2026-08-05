//! Anthropic Messages protocol.
//! From reference/packages/llm/src/protocols/anthropic-messages.ts

use std::any::Any;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::utils::cache as Cache;
use super::utils::lifecycle;
use super::utils::tool_schema::ToolSchemaProjection;
use super::utils::tool_stream::{self, ToolStream};
use crate::provider_error::is_context_overflow;
use crate::route::protocol::ProtocolStream;
use crate::route::Protocol;
use crate::schema::messages::{
    ContentPart, MediaPart, Message, ToolCallPart, ToolContent, ToolDefinition, ToolResultPart,
};
use crate::schema::{
    CacheHint, FinishReason, LlmError, LlmEvent, LlmRequest, ToolChoiceType, Usage,
};
use crate::shared;

pub const ADAPTER: &str = "anthropic-messages";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
pub const PATH: &str = "/messages";

const ANTHROPIC_BREAKPOINT_CAP: usize = 4;

// =============================================================================
// Streaming Event Schema
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AnthropicUsage {
    #[serde(rename = "input_tokens")]
    input_tokens: Option<i64>,
    #[serde(rename = "output_tokens")]
    output_tokens: Option<i64>,
    #[serde(rename = "cache_creation_input_tokens")]
    cache_creation_input_tokens: Option<Option<i64>>,
    #[serde(rename = "cache_read_input_tokens")]
    cache_read_input_tokens: Option<Option<i64>>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct AnthropicStreamBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    id: Option<String>,
    name: Option<String>,
    text: Option<String>,
    thinking: Option<String>,
    signature: Option<String>,
    input: Option<Value>,
    #[serde(rename = "tool_use_id")]
    tool_use_id: Option<String>,
    content: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AnthropicStreamDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    thinking: Option<String>,
    #[serde(rename = "partial_json")]
    partial_json: Option<String>,
    signature: Option<String>,
    #[serde(rename = "stop_reason")]
    stop_reason: Option<Option<String>>,
    #[serde(rename = "stop_sequence")]
    stop_sequence: Option<Option<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicErrorMessage {
    #[serde(rename = "type")]
    message_type: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicMessageStart {
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicEvent {
    #[serde(rename = "type")]
    event_type: String,
    index: Option<i64>,
    message: Option<AnthropicMessageStart>,
    #[serde(rename = "content_block")]
    content_block: Option<AnthropicStreamBlock>,
    delta: Option<AnthropicStreamDelta>,
    usage: Option<AnthropicUsage>,
    error: Option<AnthropicErrorMessage>,
}

// =============================================================================
// Parser State
// =============================================================================

#[derive(Clone)]
pub struct ParserState {
    pub tools: tool_stream::State<i64>,
    pub usage: Option<Usage>,
    pub lifecycle: lifecycle::State,
}

// =============================================================================
// Request Lowering
// =============================================================================

fn cache_control(breakpoints: &mut Cache::Breakpoints, cache: Option<&CacheHint>) -> Option<Value> {
    let cache = cache?;
    if !matches!(
        cache.kind,
        crate::schema::CacheHintType::Ephemeral | crate::schema::CacheHintType::Persistent
    ) {
        return None;
    }
    if breakpoints.remaining <= 0 {
        breakpoints.dropped += 1;
        return None;
    }
    breakpoints.remaining -= 1;
    if Cache::ttl_bucket(cache.ttl_seconds) == Some("1h") {
        Some(Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("ephemeral".to_string())),
            ("ttl".to_string(), Value::String("1h".to_string())),
        ])))
    } else {
        Some(Value::Object(Map::from_iter([(
            "type".to_string(),
            Value::String("ephemeral".to_string()),
        )])))
    }
}

fn anthropic_metadata(metadata: Map<String, Value>) -> crate::schema::ProviderMetadata {
    crate::schema::ProviderMetadata::from_iter([("anthropic".to_string(), metadata)])
}

fn signature_from_metadata(metadata: Option<&crate::schema::ProviderMetadata>) -> Option<String> {
    let anthropic = metadata?.get("anthropic")?;
    anthropic
        .get("signature")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

fn lower_tool(
    breakpoints: &mut Cache::Breakpoints,
    tool: &ToolDefinition,
    input_schema: &Value,
) -> Value {
    let mut obj = Map::new();
    obj.insert("name".to_string(), Value::String(tool.name.clone()));
    obj.insert(
        "description".to_string(),
        Value::String(tool.description.clone()),
    );
    obj.insert("input_schema".to_string(), input_schema.clone());
    crate::jset_opt!(
        obj,
        "cache_control",
        cache_control(breakpoints, tool.cache.as_ref())
    );
    Value::Object(obj)
}

fn lower_tool_choice(tool_choice: &crate::schema::ToolChoice) -> Result<Value, LlmError> {
    match tool_choice.kind {
        ToolChoiceType::Auto => Ok(Value::Object(Map::from_iter([(
            "type".to_string(),
            Value::String("auto".to_string()),
        )]))),
        ToolChoiceType::None => Ok(Value::Null),
        ToolChoiceType::Required => Ok(Value::Object(Map::from_iter([(
            "type".to_string(),
            Value::String("any".to_string()),
        )]))),
        ToolChoiceType::Tool => {
            let Some(name) = &tool_choice.name else {
                return Err(shared::invalid_request(
                    "Anthropic Messages tool choice requires a tool name",
                ));
            };
            Ok(Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("tool".to_string())),
                ("name".to_string(), Value::String(name.clone())),
            ])))
        }
    }
}

fn lower_tool_call(part: &ToolCallPart) -> Value {
    Value::Object(Map::from_iter([
        ("type".to_string(), Value::String("tool_use".to_string())),
        ("id".to_string(), Value::String(part.id.clone())),
        ("name".to_string(), Value::String(part.name.clone())),
        ("input".to_string(), part.input.clone()),
    ]))
}

fn lower_server_tool_call(part: &ToolCallPart) -> Value {
    Value::Object(Map::from_iter([
        (
            "type".to_string(),
            Value::String("server_tool_use".to_string()),
        ),
        ("id".to_string(), Value::String(part.id.clone())),
        ("name".to_string(), Value::String(part.name.clone())),
        ("input".to_string(), part.input.clone()),
    ]))
}

fn server_tool_result_type(name: &str) -> Option<&'static str> {
    match name {
        "web_search" => Some("web_search_tool_result"),
        "code_execution" => Some("code_execution_tool_result"),
        "web_fetch" => Some("web_fetch_tool_result"),
        _ => None,
    }
}

fn lower_server_tool_result(part: &ToolResultPart) -> Result<Value, LlmError> {
    let Some(wire_type) = server_tool_result_type(&part.name) else {
        return Err(shared::invalid_request(format!(
            "Anthropic Messages does not know how to round-trip server tool result for {}",
            part.name
        )));
    };
    let value = match &part.result {
        crate::schema::ToolResultValue::Json { value }
        | crate::schema::ToolResultValue::Text { value }
        | crate::schema::ToolResultValue::Error { value } => value.clone(),
        crate::schema::ToolResultValue::Content { value } => {
            serde_json::to_value(value).unwrap_or(Value::Null)
        }
    };
    Ok(Value::Object(Map::from_iter([
        ("type".to_string(), Value::String(wire_type.to_string())),
        ("tool_use_id".to_string(), Value::String(part.id.clone())),
        ("content".to_string(), value),
    ])))
}

fn lower_image(part: &MediaPart) -> Result<Value, LlmError> {
    let supported: std::collections::HashSet<String> =
        shared::IMAGE_MIMES.iter().map(|s| s.to_string()).collect();
    let media = shared::validate_media("Anthropic Messages", part, &supported)?;
    Ok(Value::Object(Map::from_iter([
        ("type".to_string(), Value::String("image".to_string())),
        (
            "source".to_string(),
            Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("base64".to_string())),
                ("media_type".to_string(), Value::String(media.mime.clone())),
                ("data".to_string(), Value::String(media.base64.clone())),
            ])),
        ),
    ])))
}

fn lower_tool_result_content_item(item: &ToolContent) -> Result<Value, LlmError> {
    match item {
        ToolContent::Text { text } => Ok(Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("text".to_string())),
            ("text".to_string(), Value::String(text.clone())),
        ]))),
        ToolContent::File { .. } => {
            let supported: std::collections::HashSet<String> =
                shared::IMAGE_MIMES.iter().map(|s| s.to_string()).collect();
            let media =
                shared::validate_tool_file("Anthropic Messages", &tool_file(item), &supported)?;
            Ok(Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("image".to_string())),
                (
                    "source".to_string(),
                    Value::Object(Map::from_iter([
                        ("type".to_string(), Value::String("base64".to_string())),
                        ("media_type".to_string(), Value::String(media.mime)),
                        ("data".to_string(), Value::String(media.base64)),
                    ])),
                ),
            ])))
        }
    }
}

fn lower_tool_result_content(part: &ToolResultPart) -> Result<Value, LlmError> {
    match &part.result {
        crate::schema::ToolResultValue::Content { value } => Ok(Value::Array(
            value
                .iter()
                .map(lower_tool_result_content_item)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        _ => Ok(Value::String(shared::tool_result_text(part))),
    }
}

fn supports_native_system_updates(request: &LlmRequest) -> bool {
    request.model.id.0 == "claude-opus-4-8"
}

fn ends_in_server_tool_use(message: &Message) -> bool {
    let Some(last) = message.content.last() else {
        return false;
    };
    matches!(last, ContentPart::ToolCall { provider_executed, .. } if *provider_executed == Some(true))
}

fn can_use_native_system_update(messages: &[Message], index: usize) -> bool {
    let previous = index.checked_sub(1).and_then(|i| messages.get(i));
    let next = messages.get(index + 1);
    let previous_ok = match previous {
        Some(previous) => {
            previous.role != crate::schema::MessageRole::System
                && (previous.role == crate::schema::MessageRole::User
                    || previous.role == crate::schema::MessageRole::Tool
                    || ends_in_server_tool_use(previous))
        }
        None => false,
    };
    let next_ok = match next {
        None => true,
        Some(next) => next.role == crate::schema::MessageRole::Assistant,
    };
    previous_ok && next_ok
}

fn splits_local_tool_results(messages: &[Message], index: usize) -> bool {
    let mut pending = BTreeSet::new();
    for message in &messages[..index] {
        for part in &message.content {
            if message.role == crate::schema::MessageRole::Assistant {
                if let ContentPart::ToolCall {
                    id,
                    provider_executed,
                    ..
                } = part
                {
                    if *provider_executed != Some(true) {
                        pending.insert(id.clone());
                    }
                }
            }
            if message.role == crate::schema::MessageRole::Tool {
                if let ContentPart::ToolResult { id, .. } = part {
                    pending.remove(id);
                }
            }
        }
    }
    !pending.is_empty()
}

fn lower_native_system_update(
    breakpoints: &mut Cache::Breakpoints,
    message: &Message,
) -> Result<Value, LlmError> {
    let content = shared::system_update_text("Anthropic Messages", message)?;
    let blocks: Vec<Value> = content
        .iter()
        .map(|part| {
            let mut obj = Map::new();
            obj.insert("type".to_string(), Value::String("text".to_string()));
            obj.insert("text".to_string(), Value::String(part.text.clone()));
            crate::jset_opt!(
                obj,
                "cache_control",
                cache_control(breakpoints, part.cache.as_ref())
            );
            Value::Object(obj)
        })
        .collect();
    Ok(Value::Object(Map::from_iter([
        ("role".to_string(), Value::String("system".to_string())),
        ("content".to_string(), Value::Array(blocks)),
    ])))
}

fn lower_messages(
    request: &LlmRequest,
    breakpoints: &mut Cache::Breakpoints,
) -> Result<Vec<Value>, LlmError> {
    let mut messages: Vec<Value> = Vec::new();

    for (index, message) in request.messages.iter().enumerate() {
        if message.role == crate::schema::MessageRole::System {
            if splits_local_tool_results(&request.messages, index) {
                return Err(shared::invalid_request(
                    "Anthropic Messages system updates cannot split a local tool call from its tool result",
                ));
            }
            if supports_native_system_updates(request)
                && can_use_native_system_update(&request.messages, index)
            {
                messages.push(lower_native_system_update(breakpoints, message)?);
                continue;
            }
            let part = shared::wrapped_system_update("Anthropic Messages", message)?;
            let mut block = Map::new();
            block.insert("type".to_string(), Value::String("text".to_string()));
            block.insert("text".to_string(), Value::String(part.text.clone()));
            crate::jset_opt!(
                block,
                "cache_control",
                cache_control(breakpoints, part.cache.as_ref())
            );
            let previous = messages.last().cloned();
            if let Some(Value::Object(prev)) = previous {
                if prev.get("role").and_then(Value::as_str) == Some("user") {
                    let mut next = prev.clone();
                    if let Some(Value::Array(content)) = next.get_mut("content") {
                        content.push(Value::Object(block));
                    }
                    *messages.last_mut().unwrap() = Value::Object(next);
                    continue;
                }
            }
            messages.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                (
                    "content".to_string(),
                    Value::Array(vec![Value::Object(block)]),
                ),
            ])));
            continue;
        }

        if message.role == crate::schema::MessageRole::User {
            let mut content: Vec<Value> = Vec::new();
            for part in &message.content {
                match part {
                    ContentPart::Text { text, cache, .. } => {
                        let mut block = Map::new();
                        block.insert("type".to_string(), Value::String("text".to_string()));
                        block.insert("text".to_string(), Value::String(text.clone()));
                        crate::jset_opt!(
                            block,
                            "cache_control",
                            cache_control(breakpoints, cache.as_ref())
                        );
                        content.push(Value::Object(block));
                    }
                    ContentPart::Media { .. } => content.push(lower_image(&media_part(part))?),
                    _ => {
                        return Err(shared::unsupported(
                            "Anthropic Messages",
                            "user",
                            &["text", "media"],
                        ));
                    }
                }
            }
            messages.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                ("content".to_string(), Value::Array(content)),
            ])));
            continue;
        }

        if message.role == crate::schema::MessageRole::Assistant {
            let mut content: Vec<Value> = Vec::new();
            for part in &message.content {
                match part {
                    ContentPart::Text { text, cache, .. } => {
                        let mut block = Map::new();
                        block.insert("type".to_string(), Value::String("text".to_string()));
                        block.insert("text".to_string(), Value::String(text.clone()));
                        crate::jset_opt!(
                            block,
                            "cache_control",
                            cache_control(breakpoints, cache.as_ref())
                        );
                        content.push(Value::Object(block));
                    }
                    ContentPart::Reasoning {
                        text,
                        encrypted,
                        provider_metadata,
                        ..
                    } => {
                        let mut block = Map::new();
                        block.insert("type".to_string(), Value::String("thinking".to_string()));
                        block.insert("thinking".to_string(), Value::String(text.clone()));
                        let signature = encrypted
                            .clone()
                            .or_else(|| signature_from_metadata(provider_metadata.as_ref()));
                        crate::jset_opt!(block, "signature", signature);
                        content.push(Value::Object(block));
                    }
                    ContentPart::ToolCall {
                        provider_executed, ..
                    } => {
                        let part = tool_call_part(part);
                        content.push(if *provider_executed == Some(true) {
                            lower_server_tool_call(&part)
                        } else {
                            lower_tool_call(&part)
                        });
                    }
                    ContentPart::ToolResult {
                        provider_executed, ..
                    } if *provider_executed == Some(true) => {
                        content.push(lower_server_tool_result(&tool_result_part(part))?);
                    }
                    _ => {
                        return Err(shared::invalid_request(
                            "Anthropic Messages assistant messages only support text, reasoning, and tool-call content for now",
                        ));
                    }
                }
            }
            messages.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("assistant".to_string())),
                ("content".to_string(), Value::Array(content)),
            ])));
            continue;
        }

        let mut content: Vec<Value> = Vec::new();
        for part in &message.content {
            let ContentPart::ToolResult { .. } = part else {
                return Err(shared::unsupported(
                    "Anthropic Messages",
                    "tool",
                    &["tool-result"],
                ));
            };
            let part = tool_result_part(part);
            let mut block = Map::new();
            block.insert("type".to_string(), Value::String("tool_result".to_string()));
            block.insert("tool_use_id".to_string(), Value::String(part.id.clone()));
            block.insert("content".to_string(), lower_tool_result_content(&part)?);
            if part.result.is_error() {
                block.insert("is_error".to_string(), Value::Bool(true));
            }
            crate::jset_opt!(
                block,
                "cache_control",
                cache_control(breakpoints, part.cache.as_ref())
            );
            content.push(Value::Object(block));
        }
        messages.push(Value::Object(Map::from_iter([
            ("role".to_string(), Value::String("user".to_string())),
            ("content".to_string(), Value::Array(content)),
        ])));
    }

    Ok(messages)
}

fn lower_thinking(request: &LlmRequest) -> Result<Value, LlmError> {
    let thinking = request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("anthropic"))
        .and_then(|options| options.get("thinking"));
    let Some(thinking) = thinking else {
        return Ok(Value::Null);
    };
    if !shared::is_record(thinking)
        || thinking.get("type").and_then(Value::as_str) != Some("enabled")
    {
        return Ok(Value::Null);
    }
    let budget = thinking
        .get("budgetTokens")
        .and_then(Value::as_i64)
        .or_else(|| thinking.get("budget_tokens").and_then(Value::as_i64));
    let Some(budget) = budget else {
        return Err(shared::invalid_request(
            "Anthropic thinking provider option requires budgetTokens",
        ));
    };
    Ok(Value::Object(Map::from_iter([
        ("type".to_string(), Value::String("enabled".to_string())),
        ("budget_tokens".to_string(), Value::Number(budget.into())),
    ])))
}

/// `AnthropicMessages.fromRequest`.
/// From reference/packages/llm/src/protocols/anthropic-messages.ts (`fromRequest`)
pub fn from_request(request: &LlmRequest) -> Result<Value, LlmError> {
    let tool_choice = match &request.tool_choice {
        Some(tool_choice) => {
            let lowered = lower_tool_choice(tool_choice)?;
            if lowered.is_null() {
                None
            } else {
                Some(lowered)
            }
        }
        None => None,
    };
    let generation = request.generation.clone();
    let tool_schema_compatibility = request
        .model
        .compatibility
        .as_ref()
        .and_then(|c| c.tool_schema);
    let output_limit = request
        .model
        .defaults
        .as_ref()
        .and_then(|d| d.limits.as_ref())
        .and_then(|l| l.output)
        .or_else(|| {
            request
                .model
                .route
                .defaults
                .limits
                .as_ref()
                .and_then(|l| l.output)
        })
        .unwrap_or(4096);
    let mut breakpoints = Cache::new_breakpoints(ANTHROPIC_BREAKPOINT_CAP);

    let tools = if request.tools.is_empty()
        || request
            .tool_choice
            .as_ref()
            .map(|tc| tc.kind == ToolChoiceType::None)
            .unwrap_or(false)
    {
        None
    } else {
        Some(
            request
                .tools
                .iter()
                .map(|tool| {
                    lower_tool(
                        &mut breakpoints,
                        tool,
                        &ToolSchemaProjection::model_compatibility(
                            &tool.input_schema,
                            tool_schema_compatibility,
                        ),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let system = if request.system.is_empty() {
        None
    } else {
        Some(
            request
                .system
                .iter()
                .map(|part| {
                    let mut block = Map::new();
                    block.insert("type".to_string(), Value::String("text".to_string()));
                    block.insert("text".to_string(), Value::String(part.text.clone()));
                    crate::jset_opt!(
                        block,
                        "cache_control",
                        cache_control(&mut breakpoints, part.cache.as_ref())
                    );
                    Value::Object(block)
                })
                .collect::<Vec<_>>(),
        )
    };
    let messages = lower_messages(request, &mut breakpoints)?;
    if breakpoints.dropped > 0 {
        tracing::warn!(
            "Anthropic Messages: dropped {} cache breakpoint(s); the API allows at most {} per request.",
            breakpoints.dropped,
            ANTHROPIC_BREAKPOINT_CAP
        );
    }

    let mut body = Map::new();
    body.insert(
        "model".to_string(),
        Value::String(request.model.id.0.clone()),
    );
    crate::jset_opt!(body, "system", system);
    body.insert("messages".to_string(), Value::Array(messages));
    crate::jset_opt!(body, "tools", tools);
    crate::jset_opt!(body, "tool_choice", tool_choice);
    body.insert("stream".to_string(), Value::Bool(true));
    body.insert(
        "max_tokens".to_string(),
        Value::Number(
            generation
                .as_ref()
                .and_then(|g| g.max_tokens)
                .unwrap_or(output_limit)
                .into(),
        ),
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
    crate::jset_opt!(body, "top_k", generation.as_ref().and_then(|g| g.top_k));
    crate::jset_opt!(
        body,
        "stop_sequences",
        generation.as_ref().and_then(|g| g.stop.clone())
    );
    let thinking = lower_thinking(request)?;
    if !thinking.is_null() {
        body.insert("thinking".to_string(), thinking);
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Stream Parsing
// =============================================================================

fn map_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("end_turn") | Some("stop_sequence") | Some("pause_turn") => FinishReason::Stop,
        Some("max_tokens") => FinishReason::Length,
        Some("tool_use") => FinishReason::ToolCalls,
        Some("refusal") => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

fn map_usage(usage: &AnthropicUsage) -> Option<Usage> {
    let usage = usage.clone();
    let non_cached = usage.input_tokens;
    let cache_read = usage.cache_read_input_tokens.flatten();
    let cache_write = usage.cache_creation_input_tokens.flatten();
    let input_tokens = shared::sum_tokens(&[non_cached, cache_read, cache_write]);
    let raw = serde_json::to_value(&usage).unwrap_or(Value::Null);
    Some(Usage {
        input_tokens,
        output_tokens: usage.output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cache_read,
        cache_write_input_tokens: cache_write,
        reasoning_tokens: None,
        total_tokens: shared::total_tokens(input_tokens, usage.output_tokens, None),
        provider_metadata: Some(crate::schema::ProviderMetadata::from_iter([(
            "anthropic".to_string(),
            raw.as_object().cloned().unwrap_or_default(),
        )])),
    })
}

fn merge_usage(left: &Usage, right: &Usage) -> Usage {
    let non_cached = right
        .non_cached_input_tokens
        .or(left.non_cached_input_tokens);
    let cache_read = right
        .cache_read_input_tokens
        .or(left.cache_read_input_tokens);
    let cache_write = right
        .cache_write_input_tokens
        .or(left.cache_write_input_tokens);
    let input_tokens = shared::sum_tokens(&[non_cached, cache_read, cache_write]);
    let output_tokens = right.output_tokens.or(left.output_tokens);
    let mut anthropic = Map::new();
    if let Some(left_anthropic) = left
        .provider_metadata
        .as_ref()
        .and_then(|m| m.get("anthropic"))
    {
        for (k, v) in left_anthropic {
            anthropic.insert(k.clone(), v.clone());
        }
    }
    if let Some(right_anthropic) = right
        .provider_metadata
        .as_ref()
        .and_then(|m| m.get("anthropic"))
    {
        for (k, v) in right_anthropic {
            anthropic.insert(k.clone(), v.clone());
        }
    }
    Usage {
        input_tokens,
        output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cache_read,
        cache_write_input_tokens: cache_write,
        reasoning_tokens: None,
        total_tokens: shared::total_tokens(input_tokens, output_tokens, None),
        provider_metadata: Some(crate::schema::ProviderMetadata::from_iter([(
            "anthropic".to_string(),
            anthropic,
        )])),
    }
}

const SERVER_TOOL_RESULT_NAMES: [(&str, &str); 3] = [
    ("web_search_tool_result", "web_search"),
    ("code_execution_tool_result", "code_execution"),
    ("web_fetch_tool_result", "web_fetch"),
];

fn server_tool_result_event(block: &AnthropicStreamBlock) -> Option<LlmEvent> {
    let wire_type = block.block_type.as_deref()?;
    let name = SERVER_TOOL_RESULT_NAMES
        .iter()
        .find(|(kind, _)| *kind == wire_type)
        .map(|(_, name)| *name)?;
    let error_payload = block
        .content
        .as_ref()
        .map(|content| {
            if let Some(obj) = content.as_object() {
                if let Some(kind) = obj.get("type").and_then(Value::as_str) {
                    return kind.to_string();
                }
            }
            String::new()
        })
        .unwrap_or_default();
    let is_error = error_payload.ends_with("_tool_result_error");
    let result = if is_error {
        crate::schema::ToolResultValue::Error {
            value: block.content.clone().unwrap_or(Value::Null),
        }
    } else {
        crate::schema::ToolResultValue::Json {
            value: block.content.clone().unwrap_or(Value::Null),
        }
    };
    Some(LlmEvent::ToolResult {
        id: block.tool_use_id.clone().unwrap_or_default(),
        name: name.to_string(),
        result,
        output: None,
        provider_executed: Some(true),
        provider_metadata: Some(anthropic_metadata(Map::from_iter([(
            "blockType".to_string(),
            Value::String(wire_type.to_string()),
        )]))),
    })
}

type StepResult = (ParserState, Vec<LlmEvent>);

fn on_message_start(state: &ParserState, event: &AnthropicEvent) -> StepResult {
    let usage = event
        .message
        .as_ref()
        .and_then(|m| m.usage.as_ref())
        .and_then(map_usage);
    let mut next = state.clone();
    if let Some(usage) = usage {
        next.usage = Some(match (&state.usage, &next.usage) {
            (Some(left), _) => merge_usage(left, &usage),
            (None, _) => usage,
        });
    }
    (next, vec![])
}

fn on_content_block_start(state: &ParserState, event: &AnthropicEvent) -> StepResult {
    let Some(block) = &event.content_block else {
        return (state.clone(), vec![]);
    };

    if block.block_type.as_deref() == Some("tool_use")
        || block.block_type.as_deref() == Some("server_tool_use")
    {
        if let Some(index) = event.index {
            let mut events = Vec::new();
            let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
            let id = block.id.clone().unwrap_or_else(|| index.to_string());
            let name = block.name.clone().unwrap_or_default();
            let mut next = state.clone();
            next.lifecycle = lifecycle;
            next.tools = ToolStream::start(
                &state.tools,
                index,
                tool_stream::PendingToolInput {
                    id: id.clone(),
                    name: name.clone(),
                    input: None,
                    provider_executed: if block.block_type.as_deref() == Some("server_tool_use") {
                        Some(true)
                    } else {
                        None
                    },
                    provider_metadata: None,
                },
            );
            events.push(LlmEvent::ToolInputStart {
                id: id.clone(),
                name,
                provider_metadata: None,
            });
            return (next, events);
        }
    }

    if block.block_type.as_deref() == Some("text") && block.text.is_some() {
        let mut events = Vec::new();
        let text = block.text.clone().unwrap_or_default();
        let mut next = state.clone();
        next.lifecycle = lifecycle::text_delta(
            &state.lifecycle,
            &mut events,
            &format!("text-{}", event.index.unwrap_or(0)),
            &text,
        );
        return (next, events);
    }

    if block.block_type.as_deref() == Some("thinking") && block.thinking.is_some() {
        let mut events = Vec::new();
        let thinking = block.thinking.clone().unwrap_or_default();
        let mut next = state.clone();
        next.lifecycle = lifecycle::reasoning_delta(
            &state.lifecycle,
            &mut events,
            &format!("reasoning-{}", event.index.unwrap_or(0)),
            &thinking,
            None,
        );
        return (next, events);
    }

    let Some(result) = server_tool_result_event(block) else {
        return (state.clone(), vec![]);
    };
    let mut events = Vec::new();
    let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
    events.push(result);
    let mut next = state.clone();
    next.lifecycle = lifecycle;
    (next, events)
}

fn on_content_block_delta(
    state: &ParserState,
    event: &AnthropicEvent,
) -> Result<StepResult, LlmError> {
    let Some(delta) = &event.delta else {
        return Ok((state.clone(), vec![]));
    };

    if delta.delta_type.as_deref() == Some("text_delta") && delta.text.is_some() {
        let mut events = Vec::new();
        let text = delta.text.clone().unwrap_or_default();
        let mut next = state.clone();
        next.lifecycle = lifecycle::text_delta(
            &state.lifecycle,
            &mut events,
            &format!("text-{}", event.index.unwrap_or(0)),
            &text,
        );
        return Ok((next, events));
    }

    if delta.delta_type.as_deref() == Some("thinking_delta") && delta.thinking.is_some() {
        let mut events = Vec::new();
        let thinking = delta.thinking.clone().unwrap_or_default();
        let mut next = state.clone();
        next.lifecycle = lifecycle::reasoning_delta(
            &state.lifecycle,
            &mut events,
            &format!("reasoning-{}", event.index.unwrap_or(0)),
            &thinking,
            None,
        );
        return Ok((next, events));
    }

    if delta.delta_type.as_deref() == Some("signature_delta") && delta.signature.is_some() {
        let mut events = Vec::new();
        let signature = delta.signature.clone().unwrap_or_default();
        let mut next = state.clone();
        next.lifecycle = lifecycle::reasoning_end(
            &state.lifecycle,
            &mut events,
            &format!("reasoning-{}", event.index.unwrap_or(0)),
            Some(&anthropic_metadata(Map::from_iter([(
                "signature".to_string(),
                Value::String(signature),
            )]))),
        );
        return Ok((next, events));
    }

    if delta.delta_type.as_deref() == Some("input_json_delta") {
        let Some(index) = event.index else {
            return Ok((state.clone(), vec![]));
        };
        let Some(partial_json) = &delta.partial_json else {
            return Ok((state.clone(), vec![]));
        };
        let result = tool_stream::append_existing(
            ADAPTER,
            &state.tools,
            &index,
            partial_json,
            "Anthropic Messages tool argument delta is missing its tool call",
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
        return Ok((next, events));
    }

    Ok((state.clone(), vec![]))
}

fn on_content_block_stop(
    state: &ParserState,
    event: &AnthropicEvent,
) -> Result<StepResult, LlmError> {
    let Some(index) = event.index else {
        return Ok((state.clone(), vec![]));
    };
    let result = tool_stream::finish(ADAPTER, &state.tools, &index)?;
    let mut events = Vec::new();
    let result_events = result.events;
    let lifecycle = if !result_events.is_empty() {
        lifecycle::step_start(&state.lifecycle, &mut events)
    } else {
        lifecycle::reasoning_end(
            &lifecycle::text_end(
                &state.lifecycle,
                &mut events,
                &format!("text-{}", index),
                None,
            ),
            &mut events,
            &format!("reasoning-{}", index),
            None,
        )
    };
    events.extend(result_events);
    let mut next = state.clone();
    next.lifecycle = lifecycle;
    next.tools = result.tools;
    Ok((next, events))
}

fn on_message_delta(state: &ParserState, event: &AnthropicEvent) -> StepResult {
    let usage = event.usage.as_ref().and_then(map_usage);
    let usage = match (&state.usage, usage) {
        (Some(left), Some(right)) => Some(merge_usage(left, &right)),
        (left, right) => left.clone().or(right),
    };
    let mut events = Vec::new();
    let stop_sequence = event
        .delta
        .as_ref()
        .and_then(|d| d.stop_sequence.clone().flatten());
    let provider_metadata = stop_sequence.as_ref().map(|seq| {
        anthropic_metadata(Map::from_iter([(
            "stopSequence".to_string(),
            Value::String(seq.clone()),
        )]))
    });
    let lifecycle = lifecycle::finish(
        &state.lifecycle,
        &mut events,
        map_finish_reason(
            event
                .delta
                .as_ref()
                .and_then(|d| d.stop_reason.clone().flatten())
                .as_deref(),
        ),
        usage.as_ref(),
        provider_metadata.as_ref(),
    );
    let mut next = state.clone();
    next.lifecycle = lifecycle;
    next.usage = usage;
    (next, events)
}

fn provider_error_message(event: &AnthropicEvent) -> String {
    let message_type = event.error.as_ref().and_then(|e| e.message_type.clone());
    let message = event.error.as_ref().and_then(|e| e.message.clone());
    match (message_type.as_deref(), message.as_deref()) {
        (Some(kind), Some(message)) if !kind.is_empty() && !message.is_empty() => {
            format!("{}: {}", kind, message)
        }
        (_, Some(message)) if !message.is_empty() => message.to_string(),
        (Some(kind), _) => kind.to_string(),
        _ => "Anthropic Messages stream error".to_string(),
    }
}

fn on_error(state: &ParserState, event: &AnthropicEvent) -> StepResult {
    let message = provider_error_message(event);
    let classification = if is_context_overflow(&message) {
        Some(crate::schema::ProviderFailureClassification::ContextOverflow)
    } else {
        None
    };
    (
        state.clone(),
        vec![LlmEvent::ProviderError {
            message,
            classification,
            retryable: None,
            provider_metadata: None,
        }],
    )
}

fn step(state: &mut ParserState, event: &AnthropicEvent) -> Result<Vec<LlmEvent>, LlmError> {
    match event.event_type.as_str() {
        "message_start" => {
            let (next, events) = on_message_start(state, event);
            *state = next;
            Ok(events)
        }
        "content_block_start" => {
            let (next, events) = on_content_block_start(state, event);
            *state = next;
            Ok(events)
        }
        "content_block_delta" => {
            let (next, events) = on_content_block_delta(state, event)?;
            *state = next;
            Ok(events)
        }
        "content_block_stop" => {
            let (next, events) = on_content_block_stop(state, event)?;
            *state = next;
            Ok(events)
        }
        "message_delta" => {
            let (next, events) = on_message_delta(state, event);
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

struct AnthropicMessagesStream;

impl ProtocolStream for AnthropicMessagesStream {
    fn initial(&self, _request: &LlmRequest) -> Box<dyn Any + Send> {
        Box::new(ParserState {
            tools: ToolStream::empty(),
            usage: None,
            lifecycle: lifecycle::initial(),
        })
    }

    fn step(
        &self,
        state: Box<dyn Any + Send>,
        event: &Value,
    ) -> Result<(Box<dyn Any + Send>, Vec<LlmEvent>), LlmError> {
        let mut state = *state
            .downcast::<ParserState>()
            .map_err(|_| shared::invalid_request("Anthropic Messages parser state mismatch"))?;
        let event: AnthropicEvent = serde_json::from_value(event.clone()).unwrap_or_default();
        let events = step(&mut state, &event)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, _event: &Value) -> bool {
        false
    }

    fn on_halt(&self, _state: Box<dyn Any + Send>) -> Vec<LlmEvent> {
        vec![]
    }
}

/// `AnthropicMessages.protocol`.
/// From reference/packages/llm/src/protocols/anthropic-messages.ts (`protocol`)
pub fn protocol() -> Protocol {
    Protocol::make(
        ADAPTER,
        Arc::new(from_request),
        Arc::new(AnthropicMessagesStream),
    )
}

/// `AnthropicMessages.route`.
/// From reference/packages/llm/src/protocols/anthropic-messages.ts (`route`)
pub fn route() -> crate::route::Route {
    crate::route::Route::make(crate::route::RouteMakeInput {
        id: ADAPTER.to_string(),
        provider: Some("anthropic".to_string()),
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
        headers: Some(Arc::new(|_request| {
            std::collections::BTreeMap::from_iter([(
                "anthropic-version".to_string(),
                "2023-06-01".to_string(),
            )])
        })),
        defaults: None,
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
