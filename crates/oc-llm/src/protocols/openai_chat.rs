//! OpenAI Chat Completions protocol.
//! From reference/packages/llm/src/protocols/openai-chat.ts

use std::any::Any;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::utils::lifecycle;
use super::utils::openai_options as OpenAIOptions;
use super::utils::tool_schema::ToolSchemaProjection;
use super::utils::tool_stream::{self, ToolStream};
use crate::route::protocol::ProtocolStream;
use crate::route::Protocol;
use crate::schema::messages::{
    ContentPart, MediaPart, Message, ReasoningPart, TextPart, ToolCallPart, ToolContent,
    ToolDefinition, ToolResultPart,
};
use crate::schema::{FinishReason, LlmError, LlmEvent, LlmRequest, ToolChoiceType, Usage};
use crate::shared;

pub const ADAPTER: &str = "openai-chat";
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const PATH: &str = "/chat/completions";

// =============================================================================
// Streaming Event Schema
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OpenAIChatUsage {
    #[serde(rename = "prompt_tokens")]
    prompt_tokens: Option<i64>,
    #[serde(rename = "completion_tokens")]
    completion_tokens: Option<i64>,
    #[serde(rename = "total_tokens")]
    total_tokens: Option<i64>,
    #[serde(rename = "prompt_tokens_details")]
    prompt_tokens_details: Option<Option<PromptTokensDetails>>,
    #[serde(rename = "completion_tokens_details")]
    completion_tokens_details: Option<Option<CompletionTokensDetails>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PromptTokensDetails {
    #[serde(rename = "cached_tokens")]
    cached_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CompletionTokensDetails {
    #[serde(rename = "reasoning_tokens")]
    reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct OpenAIChatToolCallDeltaFunction {
    name: Option<Option<String>>,
    arguments: Option<Option<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct OpenAIChatToolCallDelta {
    index: i64,
    id: Option<Option<String>>,
    function: Option<Option<OpenAIChatToolCallDeltaFunction>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct OpenAIChatDelta {
    content: Option<Option<String>>,
    #[serde(rename = "reasoning_content")]
    reasoning_content: Option<Option<String>>,
    #[serde(rename = "tool_calls")]
    tool_calls: Option<Option<Vec<OpenAIChatToolCallDelta>>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct OpenAIChatChoice {
    delta: Option<Option<OpenAIChatDelta>>,
    #[serde(rename = "finish_reason")]
    finish_reason: Option<Option<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct OpenAIChatEvent {
    #[serde(default)]
    choices: Vec<OpenAIChatChoice>,
    usage: Option<Option<OpenAIChatUsage>>,
}

// =============================================================================
// Parser State
// =============================================================================

pub struct ParserState {
    pub tools: tool_stream::State<i64>,
    pub tool_call_events: Vec<LlmEvent>,
    pub usage: Option<Usage>,
    pub finish_reason: Option<FinishReason>,
    pub lifecycle: lifecycle::State,
}

// =============================================================================
// Request Lowering
// =============================================================================

fn lower_tool(tool: &ToolDefinition, input_schema: &Value) -> Value {
    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(tool.name.clone()));
    function.insert(
        "description".to_string(),
        Value::String(tool.description.clone()),
    );
    function.insert(
        "parameters".to_string(),
        ToolSchemaProjection::open_ai(input_schema),
    );
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String("function".to_string()));
    obj.insert("function".to_string(), Value::Object(function));
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
                    "OpenAI Chat tool choice requires a tool name",
                ));
            };
            let mut function = Map::new();
            function.insert("name".to_string(), Value::String(name.clone()));
            let mut obj = Map::new();
            obj.insert("type".to_string(), Value::String("function".to_string()));
            obj.insert("function".to_string(), Value::Object(function));
            Ok(Value::Object(obj))
        }
    }
}

fn lower_tool_call(part: &ToolCallPart) -> Value {
    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(part.name.clone()));
    function.insert(
        "arguments".to_string(),
        Value::String(shared::encode_json(&part.input)),
    );
    let mut obj = Map::new();
    obj.insert("id".to_string(), Value::String(part.id.clone()));
    obj.insert("type".to_string(), Value::String("function".to_string()));
    obj.insert("function".to_string(), Value::Object(function));
    Value::Object(obj)
}

fn lower_media(part: &MediaPart) -> Result<Value, LlmError> {
    let supported: std::collections::HashSet<String> =
        shared::IMAGE_MIMES.iter().map(|s| s.to_string()).collect();
    let media = shared::validate_media("OpenAI Chat", part, &supported)?;
    let mut image_url = Map::new();
    image_url.insert("url".to_string(), Value::String(media.data_url));
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String("image_url".to_string()));
    obj.insert("image_url".to_string(), Value::Object(image_url));
    Ok(Value::Object(obj))
}

fn openai_compatible_reasoning_content(
    native: &Option<serde_json::Map<String, Value>>,
) -> Option<String> {
    let native = native.as_ref()?;
    let openai_compatible = native.get("openaiCompatible")?;
    let reasoning = openai_compatible.get("reasoning_content")?;
    reasoning.as_str().map(|s| s.to_string())
}

fn lower_user_message(message: &Message) -> Result<Value, LlmError> {
    let mut content: Vec<Value> = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text { text, .. } => {
                let mut obj = Map::new();
                obj.insert("type".to_string(), Value::String("text".to_string()));
                obj.insert("text".to_string(), Value::String(text.clone()));
                content.push(Value::Object(obj));
            }
            ContentPart::Media { .. } => content.push(lower_media(&media_part(part))?),
            _ => {
                return Err(shared::unsupported(
                    "OpenAI Chat",
                    "user",
                    &["text", "media"],
                ));
            }
        }
    }
    let all_text = content
        .iter()
        .all(|part| part.get("type").and_then(Value::as_str) == Some("text"));
    if all_text {
        let text = content
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        let mut obj = Map::new();
        obj.insert("role".to_string(), Value::String("user".to_string()));
        obj.insert("content".to_string(), Value::String(text));
        Ok(Value::Object(obj))
    } else {
        let mut obj = Map::new();
        obj.insert("role".to_string(), Value::String("user".to_string()));
        obj.insert("content".to_string(), Value::Array(content));
        Ok(Value::Object(obj))
    }
}

fn lower_assistant_message(message: &Message) -> Result<Value, LlmError> {
    let mut content: Vec<TextPart> = Vec::new();
    let mut reasoning: Vec<ReasoningPart> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text { text, .. } => {
                content.push(TextPart::make(text));
            }
            ContentPart::Reasoning { text, .. } => {
                reasoning.push(ReasoningPart {
                    part_type: "reasoning".to_string(),
                    text: text.clone(),
                    encrypted: None,
                    metadata: None,
                    provider_metadata: None,
                });
            }
            ContentPart::ToolCall { .. } => {
                tool_calls.push(lower_tool_call(&tool_call_part(part)));
            }
            _ => {
                return Err(shared::unsupported(
                    "OpenAI Chat",
                    "assistant",
                    &["text", "reasoning", "tool-call"],
                ));
            }
        }
    }
    let mut obj = Map::new();
    obj.insert("role".to_string(), Value::String("assistant".to_string()));
    if content.is_empty() {
        obj.insert("content".to_string(), Value::Null);
    } else {
        obj.insert(
            "content".to_string(),
            Value::String(shared::join_text(
                &content.iter().map(|p| p.text.clone()).collect::<Vec<_>>(),
            )),
        );
    }
    if !tool_calls.is_empty() {
        obj.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    if !reasoning.is_empty() {
        obj.insert(
            "reasoning_content".to_string(),
            Value::String(
                reasoning
                    .iter()
                    .map(|p| p.text.clone())
                    .collect::<Vec<_>>()
                    .join(""),
            ),
        );
    } else if let Some(reasoning_content) = openai_compatible_reasoning_content(&message.native) {
        obj.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning_content),
        );
    }
    Ok(Value::Object(obj))
}

struct LowerToolMessages {
    messages: Vec<Value>,
    images: Vec<Value>,
}

fn lower_tool_messages(message: &Message) -> Result<LowerToolMessages, LlmError> {
    let mut messages = Vec::new();
    let mut images = Vec::new();
    for part in &message.content {
        let ContentPart::ToolResult { .. } = part else {
            return Err(shared::unsupported("OpenAI Chat", "tool", &["tool-result"]));
        };
        let part = tool_result_part(part);
        match &part.result {
            crate::schema::ToolResultValue::Content { value } => {
                let content = value;
                let text = content
                    .iter()
                    .filter_map(|item| match item {
                        ToolContent::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let mut obj = Map::new();
                obj.insert("role".to_string(), Value::String("tool".to_string()));
                obj.insert("tool_call_id".to_string(), Value::String(part.id.clone()));
                obj.insert("content".to_string(), Value::String(text.join("\n")));
                messages.push(Value::Object(obj));
                for item in content
                    .iter()
                    .filter(|item| matches!(item, ToolContent::File { .. }))
                {
                    if let ToolContent::File { uri, mime, name } = item {
                        images.push(lower_media(&MediaPart {
                            part_type: "media".to_string(),
                            media_type: mime.clone(),
                            data: crate::schema::MediaData::Base64(uri.clone()),
                            filename: name.clone(),
                            metadata: None,
                        })?);
                    }
                }
            }
            _ => {
                let mut obj = Map::new();
                obj.insert("role".to_string(), Value::String("tool".to_string()));
                obj.insert("tool_call_id".to_string(), Value::String(part.id.clone()));
                obj.insert(
                    "content".to_string(),
                    Value::String(shared::tool_result_text(&part)),
                );
                messages.push(Value::Object(obj));
            }
        }
    }
    Ok(LowerToolMessages { messages, images })
}

fn lower_message(message: &Message) -> Result<Vec<Value>, LlmError> {
    match message.role {
        crate::schema::MessageRole::User => Ok(vec![lower_user_message(message)?]),
        crate::schema::MessageRole::Assistant => Ok(vec![lower_assistant_message(message)?]),
        _ => Ok(lower_tool_messages(message)?.messages),
    }
}

fn lower_messages(request: &LlmRequest) -> Result<Vec<Value>, LlmError> {
    let mut messages: Vec<Value> = Vec::new();
    if !request.system.is_empty() {
        let mut obj = Map::new();
        obj.insert("role".to_string(), Value::String("system".to_string()));
        obj.insert(
            "content".to_string(),
            Value::String(shared::system_part_text(&request.system)),
        );
        messages.push(Value::Object(obj));
    }
    let mut pending_images: Vec<Value> = Vec::new();
    let flush_images = |messages: &mut Vec<Value>, pending_images: &mut Vec<Value>| {
        if pending_images.is_empty() {
            return;
        }
        let images = std::mem::take(pending_images);
        let mut obj = Map::new();
        obj.insert("role".to_string(), Value::String("user".to_string()));
        obj.insert("content".to_string(), Value::Array(images));
        messages.push(Value::Object(obj));
    };

    for message in &request.messages {
        if message.role == crate::schema::MessageRole::System {
            let part = shared::wrapped_system_update("OpenAI Chat", message)?;
            if !pending_images.is_empty() {
                let mut content = std::mem::take(&mut pending_images);
                let mut text = Map::new();
                text.insert("type".to_string(), Value::String("text".to_string()));
                text.insert("text".to_string(), Value::String(part.text.clone()));
                content.push(Value::Object(text));
                let mut obj = Map::new();
                obj.insert("role".to_string(), Value::String("user".to_string()));
                obj.insert("content".to_string(), Value::Array(content));
                messages.push(Value::Object(obj));
                continue;
            }
            let previous = messages.last().cloned();
            if let Some(Value::Object(prev)) = previous {
                if prev.get("role").and_then(Value::as_str) == Some("user") {
                    if let Some(Value::String(content)) = prev.get("content") {
                        let mut next = prev.clone();
                        next.insert(
                            "content".to_string(),
                            Value::String(format!("{}\n{}", content, part.text)),
                        );
                        *messages.last_mut().unwrap() = Value::Object(next);
                        continue;
                    }
                    if prev.get("content").map(|c| c.is_array()).unwrap_or(false) {
                        let mut next = prev.clone();
                        if let Some(Value::Array(content)) = next.get_mut("content") {
                            let mut text = Map::new();
                            text.insert("type".to_string(), Value::String("text".to_string()));
                            text.insert("text".to_string(), Value::String(part.text.clone()));
                            content.push(Value::Object(text));
                        }
                        *messages.last_mut().unwrap() = Value::Object(next);
                        continue;
                    }
                }
            }
            let mut obj = Map::new();
            obj.insert("role".to_string(), Value::String("user".to_string()));
            obj.insert("content".to_string(), Value::String(part.text.clone()));
            messages.push(Value::Object(obj));
            continue;
        }
        if message.role == crate::schema::MessageRole::Tool {
            let lowered = lower_tool_messages(message)?;
            messages.extend(lowered.messages);
            pending_images.extend(lowered.images);
            continue;
        }
        flush_images(&mut messages, &mut pending_images);
        messages.extend(lower_message(message)?);
    }
    flush_images(&mut messages, &mut pending_images);
    Ok(messages)
}

fn lower_options(request: &LlmRequest) -> Result<Map<String, Value>, LlmError> {
    let mut options = Map::new();
    if let Some(store) = OpenAIOptions::store(request) {
        options.insert("store".to_string(), Value::Bool(store));
    }
    if let Some(effort) = OpenAIOptions::reasoning_effort(request) {
        if !OpenAIOptions::is_reasoning_effort(&effort) {
            return Err(shared::invalid_request(format!(
                "OpenAI Chat does not support reasoning effort {}",
                effort
            )));
        }
        options.insert("reasoning_effort".to_string(), Value::String(effort));
    }
    Ok(options)
}

/// `OpenAIChat.fromRequest`.
/// From reference/packages/llm/src/protocols/openai-chat.ts (`fromRequest`)
pub fn from_request(request: &LlmRequest) -> Result<Value, LlmError> {
    let generation = request.generation.clone();
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
    body.insert(
        "messages".to_string(),
        Value::Array(lower_messages(request)?),
    );
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
    body.insert(
        "stream_options".to_string(),
        Value::Object(Map::from_iter([(
            "include_usage".to_string(),
            Value::Bool(true),
        )])),
    );
    crate::jset_opt!(
        body,
        "max_tokens",
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
    crate::jset_opt!(
        body,
        "frequency_penalty",
        generation
            .as_ref()
            .and_then(|g| g.frequency_penalty)
            .map(shared::json_number)
    );
    crate::jset_opt!(
        body,
        "presence_penalty",
        generation
            .as_ref()
            .and_then(|g| g.presence_penalty)
            .map(shared::json_number)
    );
    crate::jset_opt!(body, "seed", generation.as_ref().and_then(|g| g.seed));
    crate::jset_opt!(
        body,
        "stop",
        generation.as_ref().and_then(|g| g.stop.clone())
    );
    for (key, value) in lower_options(request)? {
        body.insert(key, value);
    }
    Ok(Value::Object(body))
}

// =============================================================================
// Stream Parsing
// =============================================================================

fn map_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("content_filter") => FinishReason::ContentFilter,
        Some("function_call") | Some("tool_calls") => FinishReason::ToolCalls,
        _ => FinishReason::Unknown,
    }
}

fn map_usage(usage: Option<OpenAIChatUsage>) -> Option<Usage> {
    let usage = usage?;
    let cached = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.as_ref())
        .and_then(|d| d.cached_tokens);
    let reasoning = usage
        .completion_tokens_details
        .as_ref()
        .and_then(|d| d.as_ref())
        .and_then(|d| d.reasoning_tokens);
    let non_cached = shared::subtract_tokens(usage.prompt_tokens, cached);
    let raw = serde_json::to_value(&usage).unwrap_or(Value::Null);
    Some(Usage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cached,
        reasoning_tokens: reasoning,
        total_tokens: shared::total_tokens(
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
        ),
        cache_write_input_tokens: None,
        provider_metadata: Some(crate::schema::ProviderMetadata::from_iter([(
            "openai".to_string(),
            raw.as_object().cloned().unwrap_or_default(),
        )])),
    })
}

fn step(state: &mut ParserState, event: &OpenAIChatEvent) -> Result<Vec<LlmEvent>, LlmError> {
    let mut events: Vec<LlmEvent> = Vec::new();
    let usage = map_usage(event.usage.clone().flatten()).or_else(|| state.usage.clone());
    let choice = event.choices.first();
    let finish_reason = match choice.and_then(|c| c.finish_reason.clone().flatten()) {
        Some(reason) => Some(map_finish_reason(Some(&reason))),
        None => state.finish_reason,
    };
    let delta = choice.and_then(|c| c.delta.clone().flatten());
    let tool_deltas = delta
        .as_ref()
        .and_then(|d| d.tool_calls.clone().flatten())
        .unwrap_or_default();
    let mut tools = state.tools.clone();

    let mut lifecycle = state.lifecycle.clone();

    if let Some(reasoning_content) = delta
        .as_ref()
        .and_then(|d| d.reasoning_content.clone().flatten())
    {
        if !reasoning_content.is_empty() {
            lifecycle = lifecycle::reasoning_delta(
                &lifecycle,
                &mut events,
                "reasoning-0",
                &reasoning_content,
                None,
            );
        }
    }

    if let Some(content) = delta.as_ref().and_then(|d| d.content.clone().flatten()) {
        if !content.is_empty() {
            lifecycle = lifecycle::reasoning_end(&lifecycle, &mut events, "reasoning-0", None);
            lifecycle = lifecycle::text_delta(&lifecycle, &mut events, "text-0", &content);
        }
    }

    if !tool_deltas.is_empty() {
        lifecycle = lifecycle::reasoning_end(&lifecycle, &mut events, "reasoning-0", None);
    }

    for tool in &tool_deltas {
        let function = tool.function.clone().flatten();
        let delta = tool_stream::ToolDelta {
            id: tool.id.clone().flatten(),
            name: function.as_ref().and_then(|f| f.name.clone().flatten()),
            text: function
                .as_ref()
                .and_then(|f| f.arguments.clone().flatten())
                .unwrap_or_default(),
        };
        let result = tool_stream::append_or_start(
            ADAPTER,
            &tools,
            tool.index,
            delta,
            "OpenAI Chat tool call delta is missing id or name",
        )?;
        tools = result.tools;
        if !result.events.is_empty() {
            lifecycle = lifecycle::step_start(&lifecycle, &mut events);
        }
        events.extend(result.events);
    }

    let finished = if finish_reason.is_some() && state.finish_reason.is_none() && !tools.is_empty()
    {
        Some(tool_stream::finish_all(ADAPTER, &tools)?)
    } else {
        None
    };

    state.tools = finished.as_ref().map(|f| f.tools.clone()).unwrap_or(tools);
    state.tool_call_events = finished
        .map(|f| f.events)
        .unwrap_or_else(|| state.tool_call_events.clone());
    state.usage = usage;
    state.finish_reason = finish_reason;
    state.lifecycle = lifecycle;

    Ok(events)
}

fn finish_events(state: &ParserState) -> Vec<LlmEvent> {
    let mut events: Vec<LlmEvent> = Vec::new();
    let has_tool_calls = !state.tool_call_events.is_empty();
    let reason = match state.finish_reason {
        Some(FinishReason::Stop) if has_tool_calls => FinishReason::ToolCalls,
        other => other.unwrap_or(FinishReason::Unknown),
    };
    let lifecycle = if !state.tool_call_events.is_empty() {
        lifecycle::step_start(&state.lifecycle, &mut events)
    } else {
        state.lifecycle.clone()
    };
    events.extend(state.tool_call_events.iter().cloned());
    if state.finish_reason.is_some() {
        lifecycle::finish(&lifecycle, &mut events, reason, state.usage.as_ref(), None);
    }
    events
}

// =============================================================================
// Protocol
// =============================================================================

#[allow(dead_code)]
struct OpenAIChatStream {
    adapter: String,
}

impl ProtocolStream for OpenAIChatStream {
    fn initial(&self, _request: &LlmRequest) -> Box<dyn Any + Send> {
        Box::new(ParserState {
            tools: ToolStream::empty(),
            tool_call_events: Vec::new(),
            usage: None,
            finish_reason: None,
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
            .map_err(|_| shared::invalid_request("OpenAI Chat parser state mismatch"))?;
        let event: OpenAIChatEvent = serde_json::from_value(event.clone()).unwrap_or_default();
        let events = step(&mut state, &event)?;
        Ok((Box::new(state), events))
    }

    fn terminal(&self, _event: &Value) -> bool {
        false
    }

    fn on_halt(&self, state: Box<dyn Any + Send>) -> Vec<LlmEvent> {
        match state.downcast::<ParserState>() {
            Ok(state) => finish_events(&state),
            Err(_) => vec![],
        }
    }
}

/// `OpenAIChat.protocol`.
/// From reference/packages/llm/src/protocols/openai-chat.ts (`protocol`)
pub fn protocol() -> Protocol {
    Protocol::make(
        ADAPTER,
        Arc::new(|request| from_request(request)),
        Arc::new(OpenAIChatStream {
            adapter: ADAPTER.to_string(),
        }),
    )
}

/// `OpenAIChat.route`.
/// From reference/packages/llm/src/protocols/openai-chat.ts (`route`)
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

/// Build a `MediaPart` from a `ToolFileContent`.
pub fn media_from_tool_file(item: &ToolContent) -> MediaPart {
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
