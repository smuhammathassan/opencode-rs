//! Gemini generateContent protocol.
//! From reference/packages/llm/src/protocols/gemini.ts

use std::any::Any;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::utils::gemini_tool_schema as GeminiToolSchema;
use super::utils::lifecycle;
use super::utils::tool_schema::ToolSchemaProjection;
use crate::route::protocol::ProtocolStream;
use crate::route::Protocol;
use crate::schema::messages::{
    ContentPart, MediaPart, TextPart, ToolCallPart, ToolContent, ToolDefinition,
};
use crate::schema::{FinishReason, LlmError, LlmEvent, LlmRequest, ToolChoiceType, Usage};
use crate::shared;

pub const ADAPTER: &str = "gemini";
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

// =============================================================================
// Streaming Event Schema
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GeminiUsage {
    #[serde(rename = "cachedContentTokenCount")]
    cached_content_token_count: Option<i64>,
    #[serde(rename = "thoughtsTokenCount")]
    thoughts_token_count: Option<i64>,
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<i64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<i64>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct GeminiPart {
    text: Option<String>,
    thought: Option<bool>,
    #[serde(rename = "thoughtSignature")]
    thought_signature: Option<String>,
    #[serde(rename = "inlineData")]
    inline_data: Option<Value>,
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize, Default)]
struct GeminiFunctionCall {
    name: Option<String>,
    args: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct GeminiContent {
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize, Default)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct GeminiEvent {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
}

// =============================================================================
// Parser State
// =============================================================================

#[derive(Clone)]
pub struct ParserState {
    pub finish_reason: Option<String>,
    pub has_tool_calls: bool,
    pub next_tool_call_id: i64,
    pub usage: Option<Usage>,
    pub lifecycle: lifecycle::State,
    pub reasoning_signature: Option<String>,
}

// =============================================================================
// Request Lowering
// =============================================================================

fn lower_tool(tool: &ToolDefinition, input_schema: &Value) -> Value {
    let mut declaration = Map::new();
    declaration.insert("name".to_string(), Value::String(tool.name.clone()));
    declaration.insert(
        "description".to_string(),
        Value::String(tool.description.clone()),
    );
    let parameters = GeminiToolSchema::convert(input_schema);
    if let Some(parameters) = parameters {
        declaration.insert("parameters".to_string(), parameters);
    }
    Value::Object(Map::from_iter([(
        "functionDeclarations".to_string(),
        Value::Array(vec![Value::Object(declaration)]),
    )]))
}

fn lower_tool_config(tool_choice: &crate::schema::ToolChoice) -> Result<Value, LlmError> {
    let (mode, allowed): (&str, Option<Vec<String>>) = match tool_choice.kind {
        ToolChoiceType::Auto => ("AUTO", None),
        ToolChoiceType::None => ("NONE", None),
        ToolChoiceType::Required => ("ANY", None),
        ToolChoiceType::Tool => {
            let Some(name) = &tool_choice.name else {
                return Err(shared::invalid_request(
                    "Gemini tool choice requires a tool name",
                ));
            };
            ("ANY", Some(vec![name.clone()]))
        }
    };
    let mut config = Map::new();
    config.insert("mode".to_string(), Value::String(mode.to_string()));
    if let Some(allowed) = allowed {
        config.insert(
            "allowedFunctionNames".to_string(),
            Value::Array(allowed.into_iter().map(Value::String).collect()),
        );
    }
    Ok(Value::Object(Map::from_iter([(
        "functionCallingConfig".to_string(),
        Value::Object(config),
    )])))
}

fn lower_user_part(part: &ContentPart) -> Result<Value, LlmError> {
    match part {
        ContentPart::Text { text, .. } => Ok(Value::Object(Map::from_iter([(
            "text".to_string(),
            Value::String(text.clone()),
        )]))),
        ContentPart::Media { .. } => {
            let media_part = media_part(part);
            let supported: std::collections::HashSet<String> =
                shared::MEDIA_MIMES.iter().map(|s| s.to_string()).collect();
            let media = shared::validate_media("Gemini", &media_part, &supported)?;
            Ok(Value::Object(Map::from_iter([(
                "inlineData".to_string(),
                Value::Object(Map::from_iter([
                    ("mimeType".to_string(), Value::String(media.mime)),
                    ("data".to_string(), Value::String(media.base64)),
                ])),
            )])))
        }
        _ => Err(shared::invalid_request("Gemini unsupported user content")),
    }
}

fn google_metadata(metadata: Map<String, Value>) -> crate::schema::ProviderMetadata {
    crate::schema::ProviderMetadata::from_iter([("google".to_string(), metadata)])
}

fn thought_signature(
    provider_metadata: Option<&crate::schema::ProviderMetadata>,
) -> Option<String> {
    let google = provider_metadata?.get("google")?;
    google
        .get("thoughtSignature")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

fn lower_tool_call(part: &ToolCallPart) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "functionCall".to_string(),
        Value::Object(Map::from_iter([
            ("name".to_string(), Value::String(part.name.clone())),
            ("args".to_string(), part.input.clone()),
        ])),
    );
    crate::jset_opt!(
        obj,
        "thoughtSignature",
        thought_signature(part.provider_metadata.as_ref())
    );
    Value::Object(obj)
}

fn lower_messages(request: &LlmRequest) -> Result<Vec<Value>, LlmError> {
    let mut contents: Vec<Value> = Vec::new();

    for message in &request.messages {
        if message.role == crate::schema::MessageRole::System {
            let part = shared::wrapped_system_update("Gemini", message)?;
            let previous = contents.last().cloned();
            if let Some(Value::Object(prev)) = previous {
                if prev.get("role").and_then(Value::as_str) == Some("user") {
                    let mut next = prev.clone();
                    if let Some(Value::Array(parts)) = next.get_mut("parts") {
                        parts.push(Value::Object(Map::from_iter([(
                            "text".to_string(),
                            Value::String(part.text.clone()),
                        )])));
                    }
                    *contents.last_mut().unwrap() = Value::Object(next);
                    continue;
                }
            }
            contents.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                (
                    "parts".to_string(),
                    Value::Array(vec![Value::Object(Map::from_iter([(
                        "text".to_string(),
                        Value::String(part.text.clone()),
                    )]))]),
                ),
            ])));
            continue;
        }

        if message.role == crate::schema::MessageRole::User {
            let mut parts = Vec::new();
            for part in &message.content {
                if !shared::supports_content(part, &["text", "media"]) {
                    return Err(shared::unsupported("Gemini", "user", &["text", "media"]));
                }
                parts.push(lower_user_part(part)?);
            }
            contents.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                ("parts".to_string(), Value::Array(parts)),
            ])));
            continue;
        }

        if message.role == crate::schema::MessageRole::Assistant {
            let mut parts = Vec::new();
            for part in &message.content {
                match part {
                    ContentPart::Text { text, .. } => {
                        parts.push(Value::Object(Map::from_iter([(
                            "text".to_string(),
                            Value::String(text.clone()),
                        )])));
                    }
                    ContentPart::Reasoning {
                        text,
                        provider_metadata,
                        ..
                    } => {
                        let mut obj = Map::new();
                        obj.insert("text".to_string(), Value::String(text.clone()));
                        obj.insert("thought".to_string(), Value::Bool(true));
                        crate::jset_opt!(
                            obj,
                            "thoughtSignature",
                            thought_signature(provider_metadata.as_ref())
                        );
                        parts.push(Value::Object(obj));
                    }
                    ContentPart::ToolCall { .. } => {
                        parts.push(lower_tool_call(&tool_call_part(part)));
                    }
                    _ => {
                        return Err(shared::unsupported(
                            "Gemini",
                            "assistant",
                            &["text", "reasoning", "tool-call"],
                        ));
                    }
                }
            }
            contents.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("model".to_string())),
                ("parts".to_string(), Value::Array(parts)),
            ])));
            continue;
        }

        let mut parts = Vec::new();
        for part in &message.content {
            let ContentPart::ToolResult { .. } = part else {
                return Err(shared::unsupported("Gemini", "tool", &["tool-result"]));
            };
            let part = tool_result_part(part);
            match &part.result {
                crate::schema::ToolResultValue::Content { value } => {
                    let text = value
                        .iter()
                        .filter_map(|item| match item {
                            ToolContent::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    parts.push(Value::Object(Map::from_iter([(
                        "functionResponse".to_string(),
                        Value::Object(Map::from_iter([
                            ("name".to_string(), Value::String(part.name.clone())),
                            (
                                "response".to_string(),
                                Value::Object(Map::from_iter([
                                    ("name".to_string(), Value::String(part.name.clone())),
                                    ("content".to_string(), Value::String(text.join("\n"))),
                                ])),
                            ),
                        ])),
                    )])));
                    for item in value
                        .iter()
                        .filter(|item| matches!(item, ToolContent::File { .. }))
                    {
                        if let ToolContent::File { .. } = item {
                            let supported: std::collections::HashSet<String> =
                                shared::MEDIA_MIMES.iter().map(|s| s.to_string()).collect();
                            let media =
                                shared::validate_tool_file("Gemini", &tool_file(item), &supported)?;
                            parts.push(Value::Object(Map::from_iter([(
                                "inlineData".to_string(),
                                Value::Object(Map::from_iter([
                                    ("mimeType".to_string(), Value::String(media.mime)),
                                    ("data".to_string(), Value::String(media.base64)),
                                ])),
                            )])));
                        }
                    }
                }
                _ => {
                    parts.push(Value::Object(Map::from_iter([(
                        "functionResponse".to_string(),
                        Value::Object(Map::from_iter([
                            ("name".to_string(), Value::String(part.name.clone())),
                            (
                                "response".to_string(),
                                Value::Object(Map::from_iter([
                                    ("name".to_string(), Value::String(part.name.clone())),
                                    (
                                        "content".to_string(),
                                        Value::String(shared::tool_result_text(&part)),
                                    ),
                                ])),
                            ),
                        ])),
                    )])));
                }
            }
        }
        contents.push(Value::Object(Map::from_iter([
            ("role".to_string(), Value::String("user".to_string())),
            ("parts".to_string(), Value::Array(parts)),
        ])));
    }

    Ok(contents)
}

fn thinking_config(request: &LlmRequest) -> Option<Value> {
    let value = request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("gemini"))
        .and_then(|options| options.get("thinkingConfig"))?;
    if !shared::is_record(value) {
        return None;
    }
    let mut result = Map::new();
    if let Some(budget) = value.get("thinkingBudget").and_then(Value::as_i64) {
        result.insert("thinkingBudget".to_string(), Value::Number(budget.into()));
    }
    if let Some(include) = value.get("includeThoughts").and_then(Value::as_bool) {
        result.insert("includeThoughts".to_string(), Value::Bool(include));
    }
    if result.is_empty() {
        None
    } else {
        Some(Value::Object(result))
    }
}

/// `Gemini.fromRequest`.
/// From reference/packages/llm/src/protocols/gemini.ts (`fromRequest`)
pub fn from_request(request: &LlmRequest) -> Result<Value, LlmError> {
    let tools_enabled = !request.tools.is_empty()
        && request
            .tool_choice
            .as_ref()
            .map(|tc| tc.kind != ToolChoiceType::None)
            .unwrap_or(true);
    let generation = request.generation.clone();
    let tool_schema_compatibility = request
        .model
        .compatibility
        .as_ref()
        .and_then(|c| c.tool_schema);

    let mut generation_config = Map::new();
    crate::jset_opt!(
        generation_config,
        "maxOutputTokens",
        generation.as_ref().and_then(|g| g.max_tokens)
    );
    crate::jset_opt!(
        generation_config,
        "temperature",
        generation
            .as_ref()
            .and_then(|g| g.temperature)
            .map(shared::json_number)
    );
    crate::jset_opt!(
        generation_config,
        "topP",
        generation
            .as_ref()
            .and_then(|g| g.top_p)
            .map(shared::json_number)
    );
    crate::jset_opt!(
        generation_config,
        "topK",
        generation.as_ref().and_then(|g| g.top_k)
    );
    crate::jset_opt!(
        generation_config,
        "stopSequences",
        generation.as_ref().and_then(|g| g.stop.clone())
    );
    crate::jset_opt!(
        generation_config,
        "thinkingConfig",
        thinking_config(request)
    );

    let mut body = Map::new();
    body.insert(
        "contents".to_string(),
        Value::Array(lower_messages(request)?),
    );
    if !request.system.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            Value::Object(Map::from_iter([(
                "parts".to_string(),
                Value::Array(vec![Value::Object(Map::from_iter([(
                    "text".to_string(),
                    Value::String(shared::system_part_text(&request.system)),
                )]))]),
            )])),
        );
    }
    if tools_enabled {
        body.insert(
            "tools".to_string(),
            Value::Array(vec![Value::Object(Map::from_iter([(
                "functionDeclarations".to_string(),
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
            )]))]),
        );
    }
    if tools_enabled {
        if let Some(tool_choice) = &request.tool_choice {
            body.insert("toolConfig".to_string(), lower_tool_config(tool_choice)?);
        }
    }
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Stream Parsing
// =============================================================================

fn map_usage(usage: &GeminiUsage) -> Option<Usage> {
    let cached = usage.cached_content_token_count;
    let non_cached = shared::subtract_tokens(usage.prompt_token_count, cached);
    let output_tokens = usage
        .candidates_token_count
        .map(|candidates| candidates + usage.thoughts_token_count.unwrap_or(0));
    let raw = serde_json::to_value(usage).unwrap_or(Value::Null);
    Some(Usage {
        input_tokens: usage.prompt_token_count,
        output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cached,
        reasoning_tokens: usage.thoughts_token_count,
        total_tokens: shared::total_tokens(
            usage.prompt_token_count,
            output_tokens,
            usage.total_token_count,
        ),
        cache_write_input_tokens: None,
        provider_metadata: Some(crate::schema::ProviderMetadata::from_iter([(
            "google".to_string(),
            raw.as_object().cloned().unwrap_or_default(),
        )])),
    })
}

fn map_finish_reason(finish_reason: Option<&str>, has_tool_calls: bool) -> FinishReason {
    match finish_reason {
        Some("STOP") => {
            if has_tool_calls {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
        Some("MAX_TOKENS") => FinishReason::Length,
        Some("IMAGE_SAFETY")
        | Some("RECITATION")
        | Some("SAFETY")
        | Some("BLOCKLIST")
        | Some("PROHIBITED_CONTENT")
        | Some("SPII") => FinishReason::ContentFilter,
        Some("MALFORMED_FUNCTION_CALL") => FinishReason::Error,
        _ => FinishReason::Unknown,
    }
}

fn finish(state: &ParserState) -> Vec<LlmEvent> {
    if state.finish_reason.is_none() && state.usage.is_none() {
        return vec![];
    }
    let mut events = Vec::new();
    let lifecycle = match &state.reasoning_signature {
        Some(signature) => lifecycle::reasoning_end(
            &state.lifecycle,
            &mut events,
            "reasoning-0",
            Some(&google_metadata(Map::from_iter([(
                "thoughtSignature".to_string(),
                Value::String(signature.clone()),
            )]))),
        ),
        None => state.lifecycle.clone(),
    };
    lifecycle::finish(
        &lifecycle,
        &mut events,
        map_finish_reason(state.finish_reason.as_deref(), state.has_tool_calls),
        state.usage.as_ref(),
        None,
    );
    events
}

fn step(state: &mut ParserState, event: &GeminiEvent) -> Result<Vec<LlmEvent>, LlmError> {
    if let Some(usage) = &event.usage_metadata {
        if let Some(usage) = map_usage(usage) {
            state.usage = Some(usage);
        }
    }
    let candidate = event
        .candidates
        .as_ref()
        .and_then(|candidates| candidates.first());
    let Some(content) = candidate.and_then(|c| c.content.as_ref()) else {
        if let Some(finish_reason) = candidate.and_then(|c| c.finish_reason.as_deref()) {
            state.finish_reason = Some(finish_reason.to_string());
        }
        return Ok(vec![]);
    };

    let mut events = Vec::new();
    let mut has_tool_calls = state.has_tool_calls;
    let mut lifecycle = state.lifecycle.clone();
    let mut next_tool_call_id = state.next_tool_call_id;
    let mut reasoning_signature = state.reasoning_signature.clone();

    for part in &content.parts {
        if part.thought_signature.is_some() && part.thought == Some(true) {
            reasoning_signature = part.thought_signature.clone();
        }
        if let Some(text) = &part.text {
            if !text.is_empty() {
                if part.thought == Some(true) {
                    lifecycle = lifecycle::reasoning_delta(
                        &lifecycle,
                        &mut events,
                        "reasoning-0",
                        text,
                        part.thought_signature
                            .as_ref()
                            .map(|signature| {
                                google_metadata(Map::from_iter([(
                                    "thoughtSignature".to_string(),
                                    Value::String(signature.clone()),
                                )]))
                            })
                            .as_ref(),
                    );
                    continue;
                }
                lifecycle = lifecycle::reasoning_end(
                    &lifecycle,
                    &mut events,
                    "reasoning-0",
                    reasoning_signature
                        .as_ref()
                        .map(|signature| {
                            google_metadata(Map::from_iter([(
                                "thoughtSignature".to_string(),
                                Value::String(signature.clone()),
                            )]))
                        })
                        .as_ref(),
                );
                lifecycle = lifecycle::text_delta(&lifecycle, &mut events, "text-0", text);
                continue;
            }
        }

        if let Some(function_call) = &part.function_call {
            let input = function_call
                .args
                .clone()
                .unwrap_or(Value::Object(Map::new()));
            let id = format!("tool_{}", next_tool_call_id);
            next_tool_call_id += 1;
            lifecycle = lifecycle::reasoning_end(
                &lifecycle,
                &mut events,
                "reasoning-0",
                reasoning_signature
                    .as_ref()
                    .map(|signature| {
                        google_metadata(Map::from_iter([(
                            "thoughtSignature".to_string(),
                            Value::String(signature.clone()),
                        )]))
                    })
                    .as_ref(),
            );
            lifecycle = lifecycle::step_start(&lifecycle, &mut events);
            events.push(LlmEvent::ToolCall {
                id,
                name: function_call.name.clone().unwrap_or_default(),
                input,
                provider_executed: None,
                provider_metadata: part.thought_signature.as_ref().map(|signature| {
                    google_metadata(Map::from_iter([(
                        "thoughtSignature".to_string(),
                        Value::String(signature.clone()),
                    )]))
                }),
            });
            has_tool_calls = true;
        }
    }

    state.has_tool_calls = has_tool_calls;
    state.lifecycle = lifecycle;
    state.next_tool_call_id = next_tool_call_id;
    state.reasoning_signature = reasoning_signature;
    if let Some(finish_reason) = candidate.and_then(|c| c.finish_reason.as_deref()) {
        state.finish_reason = Some(finish_reason.to_string());
    }
    Ok(events)
}

// =============================================================================
// Protocol
// =============================================================================

struct GeminiStream;

impl ProtocolStream for GeminiStream {
    fn initial(&self, _request: &LlmRequest) -> Box<dyn Any + Send> {
        Box::new(ParserState {
            finish_reason: None,
            has_tool_calls: false,
            next_tool_call_id: 0,
            usage: None,
            lifecycle: lifecycle::initial(),
            reasoning_signature: None,
        })
    }

    fn step(
        &self,
        state: Box<dyn Any + Send>,
        event: &Value,
    ) -> Result<(Box<dyn Any + Send>, Vec<LlmEvent>), LlmError> {
        let mut state = *state
            .downcast::<ParserState>()
            .map_err(|_| shared::invalid_request("Gemini parser state mismatch"))?;
        let event: GeminiEvent = serde_json::from_value(event.clone()).unwrap_or_default();
        let events = step(&mut state, &event)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, _event: &Value) -> bool {
        false
    }

    fn on_halt(&self, state: Box<dyn Any + Send>) -> Vec<LlmEvent> {
        match state.downcast::<ParserState>() {
            Ok(state) => finish(&state),
            Err(_) => vec![],
        }
    }
}

/// `Gemini.protocol`.
/// From reference/packages/llm/src/protocols/gemini.ts (`protocol`)
pub fn protocol() -> Protocol {
    Protocol::make(ADAPTER, Arc::new(from_request), Arc::new(GeminiStream))
}

/// `Gemini.route`.
/// From reference/packages/llm/src/protocols/gemini.ts (`route`)
pub fn route() -> crate::route::Route {
    crate::route::Route::make(crate::route::RouteMakeInput {
        id: ADAPTER.to_string(),
        provider: Some("google".to_string()),
        protocol: protocol(),
        endpoint: crate::route::endpoint::path_dynamic(
            |input| {
                format!(
                    "/models/{}:streamGenerateContent?alt=sse",
                    input.request.model.id
                )
            },
            crate::route::EndpointOptions {
                base_url: Some(DEFAULT_BASE_URL.to_string()),
                query: None,
            },
        ),
        auth: Some(crate::route::Auth::none()),
        framing: Some(crate::route::Framing::Sse),
        headers: None,
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

fn tool_result_part(part: &ContentPart) -> crate::schema::messages::ToolResultPart {
    match part {
        ContentPart::ToolResult {
            id,
            name,
            result,
            provider_executed,
            cache,
            metadata,
            provider_metadata,
        } => crate::schema::messages::ToolResultPart {
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
fn _marker(_: &TextPart) {}
