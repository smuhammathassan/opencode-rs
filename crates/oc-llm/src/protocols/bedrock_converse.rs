//! Bedrock Converse protocol.
//! From reference/packages/llm/src/protocols/bedrock-converse.ts

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::utils::bedrock_cache as BedrockCache;
use super::utils::bedrock_media as BedrockMedia;
use super::utils::lifecycle;
use super::utils::tool_schema::ToolSchemaProjection;
use super::utils::tool_stream::{self, ToolStream};
use crate::provider_error::is_context_overflow;
use crate::route::Protocol;
use crate::route::protocol::ProtocolStream;
use crate::schema::messages::{ContentPart, MediaPart, ReasoningPart, ToolCallPart, ToolContent, ToolDefinition, ToolResultPart};
use crate::schema::CacheHint;
use crate::schema::{FinishReason, LlmError, LlmEvent, LlmRequest, ModelToolSchemaCompatibility, ToolChoiceType, Usage};
use crate::shared;

pub const ADAPTER: &str = "bedrock-converse";

// =============================================================================
// Streaming Event Schema
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BedrockUsage {
    #[serde(rename = "inputTokens")]
    input_tokens: Option<i64>,
    #[serde(rename = "outputTokens")]
    output_tokens: Option<i64>,
    #[serde(rename = "totalTokens")]
    total_tokens: Option<i64>,
    #[serde(rename = "cacheReadInputTokens")]
    cache_read_input_tokens: Option<i64>,
    #[serde(rename = "cacheWriteInputTokens")]
    cache_write_input_tokens: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockMetadata {
    usage: Option<BedrockUsage>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockMessageStop {
    #[serde(rename = "stopReason")]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockContentBlockStart {
    #[serde(rename = "contentBlockIndex")]
    content_block_index: Option<i64>,
    start: Option<BedrockStart>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockStart {
    #[serde(rename = "toolUse")]
    tool_use: Option<BedrockToolUse>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockToolUse {
    #[serde(rename = "toolUseId")]
    tool_use_id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockContentBlockDelta {
    #[serde(rename = "contentBlockIndex")]
    content_block_index: Option<i64>,
    delta: Option<BedrockDelta>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockDelta {
    text: Option<String>,
    #[serde(rename = "toolUse")]
    tool_use: Option<BedrockToolUseDelta>,
    #[serde(rename = "reasoningContent")]
    reasoning_content: Option<BedrockReasoningContent>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockToolUseDelta {
    input: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockReasoningContent {
    text: Option<String>,
    signature: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockContentBlockStop {
    #[serde(rename = "contentBlockIndex")]
    content_block_index: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockError {
    message: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct BedrockEvent {
    #[serde(rename = "contentBlockStart")]
    content_block_start: Option<BedrockContentBlockStart>,
    #[serde(rename = "contentBlockDelta")]
    content_block_delta: Option<BedrockContentBlockDelta>,
    #[serde(rename = "contentBlockStop")]
    content_block_stop: Option<BedrockContentBlockStop>,
    #[serde(rename = "messageStop")]
    message_stop: Option<BedrockMessageStop>,
    metadata: Option<BedrockMetadata>,
    #[serde(rename = "internalServerException")]
    internal_server_exception: Option<BedrockError>,
    #[serde(rename = "modelStreamErrorException")]
    model_stream_error_exception: Option<BedrockError>,
    #[serde(rename = "validationException")]
    validation_exception: Option<BedrockError>,
    #[serde(rename = "throttlingException")]
    throttling_exception: Option<BedrockError>,
    #[serde(rename = "serviceUnavailableException")]
    service_unavailable_exception: Option<BedrockError>,
}

// =============================================================================
// Parser State
// =============================================================================

#[derive(Clone)]
pub struct ParserState {
    pub tools: tool_stream::State<i64>,
    pub pending_finish: Option<PendingFinish>,
    pub has_tool_calls: bool,
    pub lifecycle: lifecycle::State,
    pub reasoning_signatures: BTreeMap<i64, String>,
}

#[derive(Debug, Clone)]
pub struct PendingFinish {
    pub reason: FinishReason,
    pub usage: Option<Usage>,
}

// =============================================================================
// Request Lowering
// =============================================================================

fn lower_tool_spec(tool: &ToolDefinition, input_schema: &Value) -> Value {
    Value::Object(Map::from_iter([(
        "toolSpec".to_string(),
        Value::Object(Map::from_iter([
            ("name".to_string(), Value::String(tool.name.clone())),
            ("description".to_string(), Value::String(tool.description.clone())),
            (
                "inputSchema".to_string(),
                Value::Object(Map::from_iter([("json".to_string(), input_schema.clone())])),
            ),
        ])),
    )]))
}

fn lower_tools(
    compatibility: Option<ModelToolSchemaCompatibility>,
    breakpoints: &mut BedrockCache::Breakpoints,
    tools: &[ToolDefinition],
) -> Vec<Value> {
    let mut result = Vec::new();
    for tool in tools {
        result.push(lower_tool_spec(tool, &ToolSchemaProjection::model_compatibility(&tool.input_schema, compatibility)));
        if let Some(cache_point) = BedrockCache::block(breakpoints, tool.cache.as_ref()) {
            result.push(cache_point);
        }
    }
    result
}

fn text_with_cache(
    breakpoints: &mut BedrockCache::Breakpoints,
    text: &str,
    cache: Option<&CacheHint>,
) -> Vec<Value> {
    let cache_point = BedrockCache::block(breakpoints, cache);
    let mut result = vec![Value::Object(Map::from_iter([("text".to_string(), Value::String(text.to_string()))]))];
    if let Some(cache_point) = cache_point {
        result.push(cache_point);
    }
    result
}

fn lower_tool_choice(tool_choice: &crate::schema::ToolChoice) -> Result<Value, LlmError> {
    match tool_choice.kind {
        ToolChoiceType::Auto => Ok(Value::Object(Map::from_iter([("auto".to_string(), Value::Object(Map::new()))]))),
        ToolChoiceType::None => Ok(Value::Null),
        ToolChoiceType::Required => Ok(Value::Object(Map::from_iter([("any".to_string(), Value::Object(Map::new()))]))),
        ToolChoiceType::Tool => {
            let Some(name) = &tool_choice.name else {
                return Err(shared::invalid_request("Bedrock Converse tool choice requires a tool name"));
            };
            Ok(Value::Object(Map::from_iter([(
                "tool".to_string(),
                Value::Object(Map::from_iter([("name".to_string(), Value::String(name.clone()))])),
            )])))
        }
    }
}

fn bedrock_metadata(metadata: Map<String, Value>) -> crate::schema::ProviderMetadata {
    crate::schema::ProviderMetadata::from_iter([("bedrock".to_string(), metadata)])
}

fn reasoning_signature(part: &ReasoningPart) -> Option<String> {
    let bedrock = part.provider_metadata.as_ref().and_then(|m| m.get("bedrock"));
    part.encrypted.clone().or_else(|| {
        bedrock.and_then(|b| b.get("signature")).and_then(Value::as_str).map(|s| s.to_string())
    })
}

fn lower_tool_call(part: &ToolCallPart) -> Value {
    Value::Object(Map::from_iter([(
        "toolUse".to_string(),
        Value::Object(Map::from_iter([
            ("toolUseId".to_string(), Value::String(part.id.clone())),
            ("name".to_string(), Value::String(part.name.clone())),
            ("input".to_string(), part.input.clone()),
        ])),
    )]))
}

fn lower_tool_result_content(part: &ToolResultPart) -> Result<Vec<Value>, LlmError> {
    match &part.result {
        crate::schema::ToolResultValue::Text { value: _ } | crate::schema::ToolResultValue::Error { value: _ } => {
            Ok(vec![Value::Object(Map::from_iter([(
                "text".to_string(),
                Value::String(shared::tool_result_text(part)),
            )]))])
        }
        crate::schema::ToolResultValue::Json { value } => {
            Ok(vec![Value::Object(Map::from_iter([("json".to_string(), value.clone())]))])
        }
        crate::schema::ToolResultValue::Content { value } => {
            let mut content = Vec::new();
            for item in value {
                match item {
                    ToolContent::Text { text } => {
                        content.push(Value::Object(Map::from_iter([("text".to_string(), Value::String(text.clone()))])));
                    }
                    ToolContent::File { .. } => {
                        let media = BedrockMedia::lower(&media_from_tool_file(item))?;
                        if !media.as_object().map(|obj| obj.contains_key("image")).unwrap_or(false) {
                            return Err(shared::invalid_request(
                                "Bedrock Converse only supports image media in tool results",
                            ));
                        }
                        content.push(media);
                    }
                }
            }
            Ok(content)
        }
    }
}

fn lower_tool_result(part: &ToolResultPart) -> Result<Value, LlmError> {
    let mut tool_result = Map::new();
    tool_result.insert("toolUseId".to_string(), Value::String(part.id.clone()));
    tool_result.insert("content".to_string(), Value::Array(lower_tool_result_content(part)?));
    tool_result.insert(
        "status".to_string(),
        Value::String(if part.result.is_error() { "error" } else { "success" }.to_string()),
    );
    Ok(Value::Object(Map::from_iter([("toolResult".to_string(), Value::Object(tool_result))])))
}

fn lower_messages(request: &LlmRequest, breakpoints: &mut BedrockCache::Breakpoints) -> Result<Vec<Value>, LlmError> {
    let mut messages: Vec<Value> = Vec::new();

    for message in &request.messages {
        if message.role == crate::schema::MessageRole::System {
            let part = shared::wrapped_system_update("Bedrock Converse", message)?;
            let content = text_with_cache(breakpoints, &part.text, part.cache.as_ref());
            let previous = messages.last().cloned();
            if let Some(Value::Object(prev)) = previous {
                if prev.get("role").and_then(Value::as_str) == Some("user") {
                    let mut next = prev.clone();
                    if let Some(Value::Array(existing)) = next.get_mut("content") {
                        existing.extend(content);
                    }
                    *messages.last_mut().unwrap() = Value::Object(next);
                    continue;
                }
            }
            messages.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                ("content".to_string(), Value::Array(content)),
            ])));
            continue;
        }

        if message.role == crate::schema::MessageRole::User {
            let mut content: Vec<Value> = Vec::new();
            for part in &message.content {
                match part {
                    ContentPart::Text { text, cache, .. } => {
                        content.extend(text_with_cache(breakpoints, text, cache.as_ref()));
                    }
                    ContentPart::Media { .. } => content.push(BedrockMedia::lower(&media_part(part))?),
                    _ => {
                        return Err(shared::unsupported("Bedrock Converse", "user", &["text", "media"]));
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
                        content.extend(text_with_cache(breakpoints, text, cache.as_ref()));
                    }
                    ContentPart::Reasoning { text, encrypted, provider_metadata, .. } => {
                        let reasoning_part = ReasoningPart {
                            part_type: "reasoning".to_string(),
                            text: text.clone(),
                            encrypted: encrypted.clone(),
                            metadata: None,
                            provider_metadata: provider_metadata.clone(),
                        };
                        let signature = reasoning_signature(&reasoning_part);
                        let mut reasoning_text = Map::new();
                        reasoning_text.insert("text".to_string(), Value::String(text.clone()));
                        crate::jset_opt!(reasoning_text, "signature", signature);
                        content.push(Value::Object(Map::from_iter([(
                            "reasoningContent".to_string(),
                            Value::Object(Map::from_iter([("reasoningText".to_string(), Value::Object(reasoning_text))])),
                        )])));
                    }
                    ContentPart::ToolCall { .. } => content.push(lower_tool_call(&tool_call_part(part))),
                    _ => {
                        return Err(shared::unsupported("Bedrock Converse", "assistant", &["text", "reasoning", "tool-call"]));
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
                return Err(shared::unsupported("Bedrock Converse", "tool", &["tool-result"]));
            };
            let part = tool_result_part(part);
            content.push(lower_tool_result(&part)?);
            if let Some(cache_point) = BedrockCache::block(breakpoints, part.cache.as_ref()) {
                content.push(cache_point);
            }
        }
        messages.push(Value::Object(Map::from_iter([
            ("role".to_string(), Value::String("user".to_string())),
            ("content".to_string(), Value::Array(content)),
        ])));
    }

    Ok(messages)
}

fn lower_system(breakpoints: &mut BedrockCache::Breakpoints, system: &[crate::schema::SystemPart]) -> Vec<Value> {
    let mut blocks = Vec::new();
    for part in system {
        blocks.extend(text_with_cache(breakpoints, &part.text, part.cache.as_ref()));
    }
    blocks
}

/// `BedrockConverse.fromRequest`.
/// From reference/packages/llm/src/protocols/bedrock-converse.ts (`fromRequest`)
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
    let mut breakpoints = BedrockCache::breakpoints();
    let tool_config = if !request.tools.is_empty() && request.tool_choice.as_ref().map(|tc| tc.kind != ToolChoiceType::None).unwrap_or(true) {
        let mut config = Map::new();
        config.insert(
            "tools".to_string(),
            Value::Array(lower_tools(request.model.compatibility.as_ref().and_then(|c| c.tool_schema), &mut breakpoints, &request.tools)),
        );
        crate::jset_opt!(config, "toolChoice", tool_choice);
        Some(Value::Object(config))
    } else {
        None
    };
    let system = if request.system.is_empty() {
        None
    } else {
        Some(Value::Array(lower_system(&mut breakpoints, &request.system)))
    };
    let messages = lower_messages(request, &mut breakpoints)?;
    if breakpoints.dropped > 0 {
        tracing::warn!(
            "Bedrock Converse: dropped {} cache breakpoint(s); the API allows at most {} per request.",
            breakpoints.dropped,
            BedrockCache::BEDROCK_BREAKPOINT_CAP
        );
    }

    let mut inference_config = Map::new();
    crate::jset_opt!(inference_config, "maxTokens", generation.as_ref().and_then(|g| g.max_tokens));
    crate::jset_opt!(inference_config, "temperature", generation.as_ref().and_then(|g| g.temperature));
    crate::jset_opt!(inference_config, "topP", generation.as_ref().and_then(|g| g.top_p));
    crate::jset_opt!(inference_config, "stopSequences", generation.as_ref().and_then(|g| {
        let stop = g.stop.clone()?;
        if stop.is_empty() { None } else { Some(stop) }
    }));

    let mut body = Map::new();
    body.insert("modelId".to_string(), Value::String(request.model.id.0.clone()));
    body.insert("messages".to_string(), Value::Array(messages));
    crate::jset_opt!(body, "system", system);
    if !inference_config.is_empty() {
        body.insert("inferenceConfig".to_string(), Value::Object(inference_config));
    }
    crate::jset_opt!(body, "toolConfig", tool_config);
    if let Some(top_k) = generation.as_ref().and_then(|g| g.top_k) {
        body.insert(
            "additionalModelRequestFields".to_string(),
            Value::Object(Map::from_iter([("top_k".to_string(), Value::Number(top_k.into()))])),
        );
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Stream Parsing
// =============================================================================

fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" | "stop_sequence" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        "content_filtered" | "guardrail_intervened" => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

fn map_usage(usage: &BedrockUsage) -> Option<Usage> {
    let cache_total = usage.cache_read_input_tokens.unwrap_or(0) + usage.cache_write_input_tokens.unwrap_or(0);
    let non_cached = shared::subtract_tokens(usage.input_tokens, Some(cache_total));
    let raw = serde_json::to_value(usage).unwrap_or(Value::Null);
    Some(Usage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: usage.cache_read_input_tokens,
        cache_write_input_tokens: usage.cache_write_input_tokens,
        reasoning_tokens: None,
        total_tokens: shared::total_tokens(usage.input_tokens, usage.output_tokens, usage.total_tokens),
        provider_metadata: Some(crate::schema::ProviderMetadata::from_iter([(
            "bedrock".to_string(),
            raw.as_object().cloned().unwrap_or_default(),
        )])),
    })
}

fn step(state: &mut ParserState, event: &BedrockEvent) -> Result<Vec<LlmEvent>, LlmError> {
    let mut events = Vec::new();

    if let Some(block) = &event.content_block_start {
        if let Some(tool_use) = block.start.as_ref().and_then(|start| start.tool_use.as_ref()) {
            let index = block.content_block_index.unwrap_or(0);
            let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
            let id = tool_use.tool_use_id.clone().unwrap_or_default();
            let name = tool_use.name.clone().unwrap_or_default();
            state.tools = ToolStream::start(&state.tools, index, tool_stream::PendingToolInput {
                id: id.clone(),
                name: name.clone(),
                input: None,
                provider_executed: None,
                provider_metadata: None,
            });
            state.lifecycle = lifecycle;
            events.push(LlmEvent::ToolInputStart { id, name, provider_metadata: None });
            return Ok(events);
        }
    }

    if let Some(block) = &event.content_block_delta {
        if let Some(text) = block.delta.as_ref().and_then(|d| d.text.clone()) {
            let index = block.content_block_index.unwrap_or(0);
            state.lifecycle = lifecycle::text_delta(&state.lifecycle, &mut events, &format!("text-{}", index), &text);
            return Ok(events);
        }
        if let Some(reasoning) = block.delta.as_ref().and_then(|d| d.reasoning_content.as_ref()) {
            let index = block.content_block_index.unwrap_or(0);
            if let Some(text) = &reasoning.text {
                state.lifecycle = lifecycle::reasoning_delta(&state.lifecycle, &mut events, &format!("reasoning-{}", index), text, None);
            }
            if let Some(signature) = &reasoning.signature {
                state.reasoning_signatures.insert(index, signature.clone());
            }
            return Ok(events);
        }
        if let Some(tool_use) = block.delta.as_ref().and_then(|d| d.tool_use.as_ref()) {
            let index = block.content_block_index.unwrap_or(0);
            let input = tool_use.input.clone().unwrap_or_default();
            let result = tool_stream::append_existing(
                ADAPTER,
                &state.tools,
                &index,
                &input,
                "Bedrock Converse tool delta is missing its tool call",
            )?;
            let lifecycle = if !result.events.is_empty() {
                lifecycle::step_start(&state.lifecycle, &mut events)
            } else {
                state.lifecycle.clone()
            };
            events.extend(result.events);
            state.lifecycle = lifecycle;
            state.tools = result.tools;
            return Ok(events);
        }
    }

    if let Some(block) = &event.content_block_stop {
        let index = block.content_block_index.unwrap_or(0);
        let result = tool_stream::finish(ADAPTER, &state.tools, &index)?;
        let result_events = result.events;
        let lifecycle = if !result_events.is_empty() {
            lifecycle::step_start(&state.lifecycle, &mut events)
        } else {
            let metadata = state.reasoning_signatures.get(&index).map(|signature| {
                bedrock_metadata(Map::from_iter([("signature".to_string(), Value::String(signature.clone()))]))
            });
            lifecycle::reasoning_end(
                &lifecycle::text_end(&state.lifecycle, &mut events, &format!("text-{}", index), None),
                &mut events,
                &format!("reasoning-{}", index),
                metadata.as_ref(),
            )
        };
        events.extend(result_events);
        state.has_tool_calls = events.iter().any(|e| matches!(e, LlmEvent::ToolCall { .. })) || state.has_tool_calls;
        state.lifecycle = lifecycle;
        state.tools = result.tools;
        state.reasoning_signatures.remove(&index);
        return Ok(events);
    }

    if let Some(message_stop) = &event.message_stop {
        state.pending_finish = Some(PendingFinish {
            reason: map_finish_reason(message_stop.stop_reason.as_deref().unwrap_or("unknown")),
            usage: state.pending_finish.as_ref().and_then(|p| p.usage.clone()),
        });
        return Ok(vec![]);
    }

    if let Some(metadata) = &event.metadata {
        let usage = metadata.usage.as_ref().and_then(map_usage);
        state.pending_finish = Some(PendingFinish {
            reason: state.pending_finish.as_ref().map(|p| p.reason).unwrap_or(FinishReason::Stop),
            usage,
        });
        return Ok(vec![]);
    }

    if event.internal_server_exception.is_some() || event.model_stream_error_exception.is_some() || event.service_unavailable_exception.is_some() {
        let message = event
            .internal_server_exception
            .as_ref()
            .or(event.model_stream_error_exception.as_ref())
            .or(event.service_unavailable_exception.as_ref())
            .and_then(|e| e.message.clone())
            .unwrap_or_else(|| "Bedrock Converse stream error".to_string());
        return Ok(vec![LlmEvent::ProviderError { message, classification: None, retryable: Some(true), provider_metadata: None }]);
    }

    if event.validation_exception.is_some() || event.throttling_exception.is_some() {
        let message = event
            .validation_exception
            .as_ref()
            .or(event.throttling_exception.as_ref())
            .and_then(|e| e.message.clone())
            .unwrap_or_else(|| "Bedrock Converse error".to_string());
        let classification = if event.validation_exception.is_some() && is_context_overflow(&message) {
            Some(crate::schema::ProviderFailureClassification::ContextOverflow)
        } else {
            None
        };
        return Ok(vec![LlmEvent::ProviderError {
            message,
            classification,
            retryable: Some(event.throttling_exception.is_some()),
            provider_metadata: None,
        }]);
    }

    Ok(vec![])
}

fn on_halt(state: &ParserState) -> Vec<LlmEvent> {
    let Some(pending_finish) = &state.pending_finish else {
        return vec![];
    };
    let mut events = Vec::new();
    let reason = if pending_finish.reason == FinishReason::Stop && state.has_tool_calls {
        FinishReason::ToolCalls
    } else {
        pending_finish.reason
    };
    lifecycle::finish(&state.lifecycle, &mut events, reason, pending_finish.usage.as_ref(), None);
    events
}

// =============================================================================
// Protocol
// =============================================================================

struct BedrockConverseStream;

impl ProtocolStream for BedrockConverseStream {
    fn initial(&self, _request: &LlmRequest) -> Box<dyn Any + Send> {
        Box::new(ParserState {
            tools: ToolStream::empty(),
            pending_finish: None,
            has_tool_calls: false,
            lifecycle: lifecycle::initial(),
            reasoning_signatures: BTreeMap::new(),
        })
    }

    fn step(
        &self,
        state: Box<dyn Any + Send>,
        event: &Value,
    ) -> Result<(Box<dyn Any + Send>, Vec<LlmEvent>), LlmError> {
        let mut state = *state.downcast::<ParserState>().map_err(|_| {
            shared::invalid_request("Bedrock Converse parser state mismatch")
        })?;
        let event: BedrockEvent = serde_json::from_value(event.clone()).unwrap_or_default();
        let events = step(&mut state, &event)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, _event: &Value) -> bool {
        false
    }

    fn on_halt(&self, state: Box<dyn Any + Send>) -> Vec<LlmEvent> {
        match state.downcast::<ParserState>() {
            Ok(state) => on_halt(&state),
            Err(_) => vec![],
        }
    }
}

/// `BedrockConverse.protocol`.
/// From reference/packages/llm/src/protocols/bedrock-converse.ts (`protocol`)
pub fn protocol() -> Protocol {
    Protocol::make(ADAPTER, Arc::new(|request| from_request(request)), Arc::new(BedrockConverseStream))
}

/// `BedrockConverse.route`.
/// From reference/packages/llm/src/protocols/bedrock-converse.ts (`route`)
pub fn route() -> crate::route::Route {
    crate::route::Route::make(crate::route::RouteMakeInput {
        id: ADAPTER.to_string(),
        provider: Some("bedrock".to_string()),
        protocol: protocol(),
        endpoint: crate::route::endpoint::path_dynamic(
            |input| format!("/model/{}/converse-stream", urlencode_model(&input.body.get("modelId").and_then(Value::as_str).unwrap_or_default())),
            crate::route::EndpointOptions::none(),
        ),
        auth: Some(crate::route::Auth::custom(|_input| {
            Err(shared::invalid_request(
                "Bedrock Converse requires either route bearer auth or AWS credentials configured on the route",
            ))
        })),
        framing: Some(crate::route::Framing::AwsEventStream),
        headers: None,
        defaults: None,
    })
}

fn urlencode_model(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

// =============================================================================
// Part accessors
// =============================================================================

fn media_part(part: &ContentPart) -> MediaPart {
    match part {
        ContentPart::Media { media_type, data, filename, metadata } => MediaPart {
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
        ContentPart::ToolCall { id, name, input, provider_executed, metadata, provider_metadata } => ToolCallPart {
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
        ContentPart::ToolResult { id, name, result, provider_executed, cache, metadata, provider_metadata } => {
            ToolResultPart {
                part_type: "tool-result".to_string(),
                id: id.clone(),
                name: name.clone(),
                result: result.clone(),
                provider_executed: *provider_executed,
                cache: cache.clone(),
                metadata: metadata.clone(),
                provider_metadata: provider_metadata.clone(),
            }
        }
        _ => unreachable!(),
    }
}

fn media_from_tool_file(item: &ToolContent) -> MediaPart {
    match item {
        ToolContent::File { uri, mime, name } => MediaPart {
            part_type: "media".to_string(),
            media_type: mime.clone(),
            data: crate::schema::MediaData::Base64(uri.clone()),
            filename: name.clone(),
            metadata: None,
        },
        _ => unreachable!(),
    }
}
