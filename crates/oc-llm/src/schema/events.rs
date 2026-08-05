//! Usage, provider events, response assembly.
//! From reference/packages/llm/src/schema/events.ts

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use super::ids::{FinishReason, ProviderMetadata};
use super::messages::{ContentPart, Message, ToolCallPart, ToolOutput, ToolResultPart, ToolResultValue};
use super::options::ModelSerializable;

/// `Usage` — token usage contract.
/// From reference/packages/llm/src/schema/events.ts (`Usage`)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(rename = "inputTokens", skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(rename = "outputTokens", skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
    #[serde(rename = "nonCachedInputTokens", skip_serializing_if = "Option::is_none")]
    pub non_cached_input_tokens: Option<i64>,
    #[serde(rename = "cacheReadInputTokens", skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i64>,
    #[serde(rename = "cacheWriteInputTokens", skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<i64>,
    #[serde(rename = "reasoningTokens", skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
    #[serde(rename = "totalTokens", skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i64>,
    #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl Usage {
    /// `usage.visibleOutputTokens`.
    pub fn visible_output_tokens(&self) -> i64 {
        (self.output_tokens.unwrap_or(0) - self.reasoning_tokens.unwrap_or(0)).max(0)
    }

    pub fn from(input: UsageInput) -> Usage {
        match input {
            UsageInput::Usage(usage) => usage,
            UsageInput::Fields(fields) => fields,
        }
    }
}

pub enum UsageInput {
    Usage(Usage),
    Fields(Usage),
}

/// `LLMEvent` — provider-neutral streaming event.
/// From reference/packages/llm/src/schema/events.ts (`LLMEvent`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LlmEvent {
    #[serde(rename = "step-start")]
    StepStart { index: i64 },
    #[serde(rename = "text-start")]
    TextStart {
        id: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "text-delta")]
    TextDelta {
        id: String,
        text: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "text-end")]
    TextEnd {
        id: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-start")]
    ReasoningStart {
        id: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-delta")]
    ReasoningDelta {
        id: String,
        text: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-end")]
    ReasoningEnd {
        id: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-input-start")]
    ToolInputStart {
        id: String,
        name: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-input-delta")]
    ToolInputDelta { id: String, name: String, text: String },
    #[serde(rename = "tool-input-end")]
    ToolInputEnd {
        id: String,
        name: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-call")]
    ToolCall {
        id: String,
        name: String,
        input: Value,
        #[serde(rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-result")]
    ToolResult {
        id: String,
        name: String,
        result: ToolResultValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<ToolOutput>,
        #[serde(rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-error")]
    ToolError {
        id: String,
        name: String,
        message: String,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "step-finish")]
    StepFinish {
        index: i64,
        reason: FinishReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "finish")]
    Finish {
        reason: FinishReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "provider-error")]
    ProviderError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        classification: Option<ProviderFailureClassification>,
        #[serde(skip_serializing_if = "Option::is_none")]
        retryable: Option<bool>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

/// `ProviderFailureClassification` — `"context-overflow"`.
/// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderFailureClassification {
    ContextOverflow,
}

impl LlmEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            LlmEvent::StepStart { .. } => "step-start",
            LlmEvent::TextStart { .. } => "text-start",
            LlmEvent::TextDelta { .. } => "text-delta",
            LlmEvent::TextEnd { .. } => "text-end",
            LlmEvent::ReasoningStart { .. } => "reasoning-start",
            LlmEvent::ReasoningDelta { .. } => "reasoning-delta",
            LlmEvent::ReasoningEnd { .. } => "reasoning-end",
            LlmEvent::ToolInputStart { .. } => "tool-input-start",
            LlmEvent::ToolInputDelta { .. } => "tool-input-delta",
            LlmEvent::ToolInputEnd { .. } => "tool-input-end",
            LlmEvent::ToolCall { .. } => "tool-call",
            LlmEvent::ToolResult { .. } => "tool-result",
            LlmEvent::ToolError { .. } => "tool-error",
            LlmEvent::StepFinish { .. } => "step-finish",
            LlmEvent::Finish { .. } => "finish",
            LlmEvent::ProviderError { .. } => "provider-error",
        }
    }

    pub fn usage(&self) -> Option<&Usage> {
        match self {
            LlmEvent::StepFinish { usage, .. } | LlmEvent::Finish { usage, .. } => usage.as_ref(),
            _ => None,
        }
    }

    pub fn text_delta(id: impl Into<String>, text: impl Into<String>) -> LlmEvent {
        LlmEvent::TextDelta { id: id.into(), text: text.into(), provider_metadata: None }
    }

    pub fn reasoning_delta(id: impl Into<String>, text: impl Into<String>) -> LlmEvent {
        LlmEvent::ReasoningDelta { id: id.into(), text: text.into(), provider_metadata: None }
    }
}

/// `PreparedRequest` — compiled request metadata.
/// From reference/packages/llm/src/schema/events.ts (`PreparedRequest`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedRequest {
    pub id: String,
    pub route: String,
    pub protocol: String,
    pub model: ModelSerializable,
    pub body: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

/// `LLMResponse` — assembled response.
/// From reference/packages/llm/src/schema/events.ts (`LLMResponse`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    pub message: Message,
    pub events: Vec<LlmEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(rename = "finishReason")]
    pub finish_reason: FinishReason,
}

impl LlmResponse {
    /// `response.text` — concatenated `text-delta` events.
    pub fn text(&self) -> String {
        response_text(&self.events)
    }

    /// `response.reasoning` — concatenated `reasoning-delta` events.
    pub fn reasoning(&self) -> String {
        response_reasoning(&self.events)
    }

    /// `response.toolCalls`.
    pub fn tool_calls(&self) -> Vec<&LlmEvent> {
        self.events.iter().filter(|e| matches!(e, LlmEvent::ToolCall { .. })).collect()
    }
}

fn response_text(events: &[LlmEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            LlmEvent::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn response_reasoning(events: &[LlmEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            LlmEvent::ReasoningDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn response_usage(events: &[LlmEvent]) -> Option<Usage> {
    events.iter().rev().find_map(LlmEvent::usage).cloned()
}

/// `ResponseState` — fold accumulator.
/// From reference/packages/llm/src/schema/events.ts (`LLMResponse.State`)
#[derive(Debug, Clone)]
pub struct ResponseState {
    pub events: Vec<LlmEvent>,
    pub message: Message,
    pub usage: Option<Usage>,
    pub finish_reason: Option<FinishReason>,
    pub text_parts: BTreeMap<String, ContentAssembly>,
    pub reasoning_parts: BTreeMap<String, ContentAssembly>,
    pub tool_inputs: BTreeMap<String, ToolInputAssembly>,
}

#[derive(Debug, Clone)]
pub struct ContentAssembly {
    pub content_index: usize,
    pub text: String,
    pub provider_metadata: Option<ProviderMetadata>,
}

#[derive(Debug, Clone)]
pub struct ToolInputAssembly {
    pub name: String,
    pub text: String,
    pub provider_metadata: Option<ProviderMetadata>,
}

/// `LLMResponse.empty`.
pub fn response_empty() -> ResponseState {
    ResponseState { message: Message::assistant(Vec::<ContentPart>::new()), events: Vec::new(), usage: None, finish_reason: None, text_parts: BTreeMap::new(), reasoning_parts: BTreeMap::new(), tool_inputs: BTreeMap::new() }
}

fn text_content(text: &str, provider_metadata: Option<&ProviderMetadata>) -> ContentPart {
    ContentPart::Text {
        text: text.to_string(),
        cache: None,
        metadata: None,
        provider_metadata: provider_metadata.cloned(),
    }
}

fn reasoning_content(text: &str, provider_metadata: Option<&ProviderMetadata>) -> ContentPart {
    ContentPart::Reasoning {
        text: text.to_string(),
        encrypted: None,
        metadata: None,
        provider_metadata: provider_metadata.cloned(),
    }
}

fn content_with(state: &ResponseState, content: Vec<ContentPart>) -> ResponseState {
    ResponseState {
        events: state.events.clone(),
        message: Message { role: super::ids::MessageRole::Assistant, content, ..state.message.clone() },
        usage: state.usage.clone(),
        finish_reason: state.finish_reason,
        text_parts: state.text_parts.clone(),
        reasoning_parts: state.reasoning_parts.clone(),
        tool_inputs: state.tool_inputs.clone(),
    }
}

fn append_content(state: &ResponseState, part: ContentPart) -> ResponseState {
    let mut content = state.message.content.clone();
    content.push(part);
    content_with(state, content)
}

fn replace_content(state: &ResponseState, index: usize, part: ContentPart) -> ResponseState {
    let mut content = state.message.content.clone();
    if let Some(slot) = content.get_mut(index) {
        *slot = part;
    }
    content_with(state, content)
}

fn ensure_text(state: &ResponseState, id: &str, provider_metadata: Option<&ProviderMetadata>) -> ResponseState {
    if state.text_parts.contains_key(id) {
        return state.clone();
    }
    let mut next = append_content(state, text_content("", provider_metadata));
    next.text_parts.insert(
        id.to_string(),
        ContentAssembly {
            content_index: state.message.content.len(),
            text: String::new(),
            provider_metadata: provider_metadata.cloned(),
        },
    );
    next
}

fn reduce_text_delta(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let LlmEvent::TextDelta { id, text, provider_metadata } = event else {
        return state.clone();
    };
    let started = ensure_text(state, id, provider_metadata.as_ref());
    let Some(current) = started.text_parts.get(id) else { return started };
    let next_text = format!("{}{}", current.text, text);
    let next_metadata = provider_metadata.clone().or_else(|| current.provider_metadata.clone());
    let with_content = replace_content(&started, current.content_index, text_content(&next_text, next_metadata.as_ref()));
    let mut next = with_content;
    next.text_parts.insert(
        id.clone(),
        ContentAssembly {
            content_index: current.content_index,
            text: next_text,
            provider_metadata: next_metadata,
        },
    );
    next
}

fn reduce_text_end(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let LlmEvent::TextEnd { id, provider_metadata } = event else {
        return state.clone();
    };
    let Some(current) = state.text_parts.get(id) else { return state.clone() };
    let next_metadata = provider_metadata.clone().or_else(|| current.provider_metadata.clone());
    let next = replace_content(state, current.content_index, text_content(&current.text, next_metadata.as_ref()));
    let mut next = next;
    next.text_parts.insert(
        id.clone(),
        ContentAssembly {
            content_index: current.content_index,
            text: current.text.clone(),
            provider_metadata: next_metadata,
        },
    );
    next
}

fn ensure_reasoning(state: &ResponseState, id: &str, provider_metadata: Option<&ProviderMetadata>) -> ResponseState {
    if state.reasoning_parts.contains_key(id) {
        return state.clone();
    }
    let mut next = append_content(state, reasoning_content("", provider_metadata));
    next.reasoning_parts.insert(
        id.to_string(),
        ContentAssembly {
            content_index: state.message.content.len(),
            text: String::new(),
            provider_metadata: provider_metadata.cloned(),
        },
    );
    next
}

fn reduce_reasoning_delta(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let LlmEvent::ReasoningDelta { id, text, provider_metadata } = event else {
        return state.clone();
    };
    let started = ensure_reasoning(state, id, provider_metadata.as_ref());
    let Some(current) = started.reasoning_parts.get(id) else { return started };
    let next_text = format!("{}{}", current.text, text);
    let next_metadata = provider_metadata.clone().or_else(|| current.provider_metadata.clone());
    let with_content = replace_content(&started, current.content_index, reasoning_content(&next_text, next_metadata.as_ref()));
    let mut next = with_content;
    next.reasoning_parts.insert(
        id.clone(),
        ContentAssembly {
            content_index: current.content_index,
            text: next_text,
            provider_metadata: next_metadata,
        },
    );
    next
}

fn reduce_reasoning_end(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let LlmEvent::ReasoningEnd { id, provider_metadata } = event else {
        return state.clone();
    };
    let Some(current) = state.reasoning_parts.get(id) else { return state.clone() };
    let next_metadata = provider_metadata.clone().or_else(|| current.provider_metadata.clone());
    let next = replace_content(state, current.content_index, reasoning_content(&current.text, next_metadata.as_ref()));
    let mut next = next;
    next.reasoning_parts.insert(
        id.clone(),
        ContentAssembly {
            content_index: current.content_index,
            text: current.text.clone(),
            provider_metadata: next_metadata,
        },
    );
    next
}

fn reduce_tool_input_start(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let LlmEvent::ToolInputStart { id, name, provider_metadata } = event else {
        return state.clone();
    };
    let mut next = state.clone();
    next.tool_inputs.insert(
        id.clone(),
        ToolInputAssembly { name: name.clone(), text: String::new(), provider_metadata: provider_metadata.clone() },
    );
    next
}

fn reduce_tool_input_delta(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let LlmEvent::ToolInputDelta { id, name, text } = event else {
        return state.clone();
    };
    let mut next = state.clone();
    let current = next
        .tool_inputs
        .get(id)
        .cloned()
        .unwrap_or_else(|| ToolInputAssembly { name: name.clone(), text: String::new(), provider_metadata: None });
    next.tool_inputs.insert(
        id.clone(),
        ToolInputAssembly { name: current.name.clone(), text: format!("{}{}", current.text, text), provider_metadata: current.provider_metadata.clone() },
    );
    next
}

fn reduce_tool_input_end(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let LlmEvent::ToolInputEnd { id, name, provider_metadata } = event else {
        return state.clone();
    };
    let mut next = state.clone();
    let current = next
        .tool_inputs
        .get(id)
        .cloned()
        .unwrap_or_else(|| ToolInputAssembly { name: name.clone(), text: String::new(), provider_metadata: None });
    let next_metadata = provider_metadata.clone().or_else(|| current.provider_metadata.clone());
    next.tool_inputs.insert(
        id.clone(),
        ToolInputAssembly { name: current.name.clone(), text: current.text.clone(), provider_metadata: next_metadata },
    );
    next
}

fn tool_call_content(event: &LlmEvent) -> ContentPart {
    let LlmEvent::ToolCall { id, name, input, provider_executed, provider_metadata } = event else {
        unreachable!()
    };
    ContentPart::from_tool_call(ToolCallPart {
        part_type: "tool-call".to_string(),
        id: id.clone(),
        name: name.clone(),
        input: input.clone(),
        provider_executed: *provider_executed,
        metadata: None,
        provider_metadata: provider_metadata.clone(),
    })
}

fn tool_result_content(event: &LlmEvent) -> ContentPart {
    let LlmEvent::ToolResult { id, name, result, provider_executed, provider_metadata, .. } = event else {
        unreachable!()
    };
    ContentPart::from_tool_result(ToolResultPart {
        part_type: "tool-result".to_string(),
        id: id.clone(),
        name: name.clone(),
        result: result.clone(),
        provider_executed: *provider_executed,
        cache: None,
        metadata: None,
        provider_metadata: provider_metadata.clone(),
    })
}

fn reduce_tool_call(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let id = match event {
        LlmEvent::ToolCall { id, .. } => id.clone(),
        _ => String::new(),
    };
    let mut next = append_content(state, tool_call_content(event));
    next.tool_inputs.remove(&id);
    next
}

/// `LLMResponse.reduce` — pure fold over one event.
pub fn response_reduce(state: &ResponseState, event: &LlmEvent) -> ResponseState {
    let mut next = state.clone();
    next.events.push(event.clone());
    match event {
        LlmEvent::Finish { usage, reason, .. } => {
            next.usage = usage.clone().or_else(|| state.usage.clone());
            next.finish_reason = Some(*reason);
        }
        LlmEvent::ProviderError { .. } => {
            next.finish_reason = Some(state.finish_reason.unwrap_or(FinishReason::Error));
        }
        other => {
            if let Some(usage) = other.usage() {
                next.usage = Some(usage.clone());
            }
        }
    }

    match event {
        LlmEvent::TextStart { id, provider_metadata } => ensure_text(&next, id, provider_metadata.as_ref()),
        LlmEvent::TextDelta { .. } => reduce_text_delta(&next, event),
        LlmEvent::TextEnd { .. } => reduce_text_end(&next, event),
        LlmEvent::ReasoningStart { id, provider_metadata } => ensure_reasoning(&next, id, provider_metadata.as_ref()),
        LlmEvent::ReasoningDelta { .. } => reduce_reasoning_delta(&next, event),
        LlmEvent::ReasoningEnd { .. } => reduce_reasoning_end(&next, event),
        LlmEvent::ToolInputStart { .. } => reduce_tool_input_start(&next, event),
        LlmEvent::ToolInputDelta { .. } => reduce_tool_input_delta(&next, event),
        LlmEvent::ToolInputEnd { .. } => reduce_tool_input_end(&next, event),
        LlmEvent::ToolCall { .. } => reduce_tool_call(&next, event),
        LlmEvent::ToolResult { .. } => append_content(&next, tool_result_content(event)),
        _ => next,
    }
}

/// `LLMResponse.complete` — build a finished response or `None`.
pub fn response_complete(state: &ResponseState) -> Option<LlmResponse> {
    match state.finish_reason {
        None => None,
        Some(reason) => Some(LlmResponse {
            message: state.message.clone(),
            events: state.events.clone(),
            usage: state.usage.clone(),
            finish_reason: reason,
        }),
    }
}

/// `LLMResponse.fromEvents`.
pub fn response_from_events(events: &[LlmEvent]) -> Option<LlmResponse> {
    let state = events.iter().fold(response_empty(), |state, event| response_reduce(&state, event));
    response_complete(&state)
}

/// `LLMResponse.text(events)`.
pub fn response_text_from(events: &[LlmEvent]) -> String {
    response_text(events)
}

/// `LLMResponse.reasoning(events)`.
pub fn response_reasoning_from(events: &[LlmEvent]) -> String {
    response_reasoning(events)
}

/// `LLMResponse.usage(events)`.
pub fn response_usage_from(events: &[LlmEvent]) -> Option<Usage> {
    response_usage(events)
}

/// `LLMResponse.toolCalls(events)`.
pub fn response_tool_calls_from(events: &[LlmEvent]) -> Vec<&LlmEvent> {
    events.iter().filter(|e| matches!(e, LlmEvent::ToolCall { .. })).collect()
}
