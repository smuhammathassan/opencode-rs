use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::llm::event::{LLMEvent, ToolOutput, ToolResultValue, Usage};
use crate::llm::message::ModelCost;
use crate::llm::{ProviderMetadata, ToolContent};
use crate::session::event::{CacheTokens, Provider, SessionEvent, Tokens};
use crate::session::message::UnknownError;
use crate::session::schema::{ModelRef, SessionID};
use crate::session::services::EventBus;
use crate::session::util::timestamp_now;

/// Input shared by every event the publisher emits.
/// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
#[derive(Debug, Clone)]
pub struct PublisherInput {
    pub session_id: SessionID,
    pub agent: String,
    pub model: ModelRef,
    pub cost: Option<ModelCost>,
    pub snapshot: Option<String>,
}

/// Failures from malformed provider streams. These correspond to `Effect.die`
/// calls in the reference.
/// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum PublishError {
    #[error("duplicate {0}")]
    Duplicate(String),
    #[error("{0} before start")]
    BeforeStart(String),
    #[error("unknown tool call: {0}")]
    UnknownTool(String),
    #[error("tool name changed for {0}")]
    NameChanged(String),
    #[error("tool result before call: {0}")]
    ResultBeforeCall(String),
    #[error("tool error before call: {0}")]
    ErrorBeforeCall(String),
    #[error("unsupported tool result: {0}")]
    UnsupportedResult(String),
    #[error("duplicate step finish")]
    DuplicateStepFinish,
}

/// Token usage as reported to Step.Ended.
/// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
#[derive(Debug, Clone, PartialEq)]
pub struct StepSettlement {
    pub finish: String,
    pub tokens: Tokens,
    pub cost: f64,
}

#[derive(Clone)]
struct ToolEntry {
    assistant_message_id: String,
    name: String,
    input_ended: bool,
    called: bool,
    settled: bool,
    provider_executed: bool,
    provider_metadata: Option<ProviderMetadata>,
}

#[derive(Default)]
struct Fragments {
    chunks: HashMap<String, Vec<String>>,
}

impl Fragments {
    fn start(&mut self, id: &str) -> Result<(), PublishError> {
        if self.chunks.contains_key(id) {
            return Err(PublishError::Duplicate(format!("start: {id}")));
        }
        self.chunks.insert(id.to_string(), Vec::new());
        Ok(())
    }

    fn append(&mut self, id: &str, value: &str) -> Result<(), PublishError> {
        let current = self
            .chunks
            .get_mut(id)
            .ok_or_else(|| PublishError::BeforeStart(format!("delta: {id}")))?;
        current.push(value.to_string());
        Ok(())
    }

    fn end(&mut self, id: &str) -> Result<String, PublishError> {
        let current = self
            .chunks
            .remove(id)
            .ok_or_else(|| PublishError::BeforeStart(format!("end: {id}")))?;
        Ok(current.join(""))
    }

    fn end_all(&mut self) -> Result<Vec<(String, String)>, PublishError> {
        let ids = self.chunks.keys().cloned().collect::<Vec<_>>();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let text = self.end(&id)?;
            out.push((id, text));
        }
        Ok(out)
    }
}

struct PublisherState {
    tools: HashMap<String, ToolEntry>,
    text: Fragments,
    reasoning: Fragments,
    tool_input: Fragments,
    assistant_message_id: Option<String>,
    assistant_active: bool,
    assistant_failed: bool,
    provider_failed: bool,
    step_settlement: Option<StepSettlement>,
}

impl PublisherState {
    fn new() -> Self {
        Self {
            tools: HashMap::new(),
            text: Fragments::default(),
            reasoning: Fragments::default(),
            tool_input: Fragments::default(),
            assistant_message_id: None,
            assistant_active: false,
            assistant_failed: false,
            provider_failed: false,
            step_settlement: None,
        }
    }
}

/// Persists one provider turn as `session.next.*` events. The reference builds
/// this with `createLLMEventPublisher`; here it is a shared, lock-guarded
/// publisher so stream processing and tool-settlement fibers can publish
/// concurrently with serialized emission.
/// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
pub struct LLMEventPublisher {
    events: Arc<dyn EventBus>,
    publication: Arc<tokio::sync::Mutex<()>>,
    state: tokio::sync::Mutex<PublisherState>,
    input: PublisherInput,
}

fn safe(value: Option<f64>) -> f64 {
    match value {
        Some(value) if value.is_finite() && value > 0.0 => value,
        _ => 0.0,
    }
}

fn tokens(usage: Option<&Usage>) -> Tokens {
    Tokens {
        input: safe(usage.and_then(|usage| usage.non_cached_input_tokens)),
        output: usage
            .map(|usage| usage.visible_output_tokens())
            .unwrap_or(0.0),
        reasoning: safe(usage.and_then(|usage| usage.reasoning_tokens)),
        cache: CacheTokens {
            read: safe(usage.and_then(|usage| usage.cache_read_input_tokens)),
            write: safe(usage.and_then(|usage| usage.cache_write_input_tokens)),
        },
    }
}

fn cost(usage: Option<&Usage>, pricing: Option<&ModelCost>) -> f64 {
    let Some(pricing) = pricing else { return 0.0 };
    let Some(usage) = usage else { return 0.0 };
    let value = usage.non_cached_input_tokens.unwrap_or(0.0) * pricing.input
        + usage.visible_output_tokens() * pricing.output
        + usage.cache_read_input_tokens.unwrap_or(0.0) * pricing.cache_read
        + usage.cache_write_input_tokens.unwrap_or(0.0) * pricing.cache_write;
    if value.is_finite() && value > 0.0 {
        value / 1_000_000.0
    } else {
        0.0
    }
}

fn record(value: &Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map.clone(),
        _ => {
            let mut map = Map::new();
            map.insert("value".to_string(), value.clone());
            map
        }
    }
}

fn message(value: &Value) -> String {
    if let Value::String(text) = value {
        return text.clone();
    }
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

struct Settled {
    structured: Map<String, Value>,
    content: Vec<ToolContent>,
}

fn settled_output(
    value: Option<ToolOutput>,
    result: &ToolResultValue,
) -> Result<Settled, UnknownError> {
    if let ToolResultValue::Error { value } = result {
        return Err(UnknownError::new(message(value)));
    }
    let settled = value.or_else(|| ToolOutput::from_result_value(result));
    let settled = match settled {
        Some(settled) => settled,
        None => {
            let fallback = serde_json::to_value(result).unwrap_or(Value::Null);
            return Err(UnknownError::new(format!(
                "Unsupported tool result: {}",
                message(&fallback)
            )));
        }
    };
    Ok(Settled {
        structured: record(&settled.structured),
        content: settled.content,
    })
}

impl LLMEventPublisher {
    pub fn new(events: Arc<dyn EventBus>, input: PublisherInput) -> Self {
        Self {
            events,
            publication: Arc::new(tokio::sync::Mutex::new(())),
            state: tokio::sync::Mutex::new(PublisherState::new()),
            input,
        }
    }

    /// Serialize event emission across concurrent publishers.
    async fn emit(&self, events: Vec<SessionEvent>) {
        if events.is_empty() {
            return;
        }
        let _permit = self.publication.lock().await;
        for event in events {
            self.events.publish(event).await;
        }
    }

    pub async fn start_assistant(&self) -> Result<String, PublishError> {
        let (id, events) = {
            let mut state = self.state.lock().await;
            if let Some(id) = &state.assistant_message_id {
                return Ok(id.clone());
            }
            let id = crate::session::util::ascending();
            state.assistant_message_id = Some(id.clone());
            state.assistant_active = true;
            let events = vec![step_started(&self.input, &id)];
            (id, events)
        };
        self.emit(events).await;
        Ok(id)
    }

    /// Transition one provider event, emitting session events. Mirrors the
    /// reference `publish` switch.
    /// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
    pub async fn publish(
        &self,
        event: &LLMEvent,
        output_paths: &[String],
    ) -> Result<(), PublishError> {
        let events = {
            let mut state = self.state.lock().await;
            handle(&mut state, &self.input, event, output_paths)?
        };
        self.emit(events).await;
        Ok(())
    }

    /// Flush any in-flight fragments (text/reasoning/tool input).
    /// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
    pub async fn flush(&self) -> Result<(), PublishError> {
        let events = {
            let mut state = self.state.lock().await;
            flush_fragments(&mut state, &self.input)?
        };
        self.emit(events).await;
        Ok(())
    }

    /// Mark the step failed and publish `Step.Failed`.
    /// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
    pub async fn fail_assistant(&self, message: &str) -> Result<(), PublishError> {
        let events = {
            let mut state = self.state.lock().await;
            fail_assistant(&mut state, &self.input, message)?
        };
        self.emit(events).await;
        Ok(())
    }

    /// Fail any unsettled tools with `message`. With `hosted_only`, only
    /// provider-executed tools are failed.
    /// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
    pub async fn fail_unsettled_tools(
        &self,
        message: &str,
        hosted_only: bool,
    ) -> Result<(), PublishError> {
        let events = {
            let mut state = self.state.lock().await;
            let mut out = Vec::new();
            let ids = state.tools.keys().cloned().collect::<Vec<_>>();
            for id in ids {
                let tool = state
                    .tools
                    .get_mut(&id)
                    .ok_or_else(|| PublishError::UnknownTool(id.clone()))?;
                if tool.settled || (hosted_only && !tool.provider_executed) {
                    continue;
                }
                tool.settled = true;
                out.push(SessionEvent::ToolFailed {
                    timestamp: timestamp_now(),
                    session_id: self.input.session_id.clone(),
                    assistant_message_id: tool.assistant_message_id.clone(),
                    call_id: id,
                    error: UnknownError::new(message),
                    result: None,
                    provider: Provider::new(tool.provider_executed, tool.provider_metadata.clone()),
                });
            }
            out
        };
        self.emit(events).await;
        Ok(())
    }

    pub fn has_active_assistant(&self) -> bool {
        self.state
            .try_lock()
            .map(|state| state.assistant_active)
            .unwrap_or(false)
    }

    pub fn has_assistant_started(&self) -> bool {
        self.state
            .try_lock()
            .map(|state| state.assistant_message_id.is_some())
            .unwrap_or(false)
    }

    pub fn has_provider_error(&self) -> bool {
        self.state
            .try_lock()
            .map(|state| state.provider_failed)
            .unwrap_or(false)
    }

    pub fn step_settlement(&self) -> Option<StepSettlement> {
        self.state
            .try_lock()
            .ok()
            .and_then(|state| state.step_settlement.clone())
    }

    /// The assistant message owning a recorded tool call.
    /// /// From reference/packages/core/src/session/runner/publish-llm-event.ts
    pub async fn assistant_message_id(&self, call_id: &str) -> Result<String, PublishError> {
        let state = self.state.lock().await;
        state
            .tools
            .get(call_id)
            .map(|tool| tool.assistant_message_id.clone())
            .ok_or_else(|| PublishError::UnknownTool(call_id.to_string()))
    }
}

fn step_started(input: &PublisherInput, assistant_message_id: &str) -> SessionEvent {
    SessionEvent::StepStarted {
        timestamp: timestamp_now(),
        session_id: input.session_id.clone(),
        assistant_message_id: assistant_message_id.to_string(),
        agent: input.agent.clone(),
        model: input.model.clone(),
        snapshot: input.snapshot.clone(),
    }
}

fn start_assistant_transition(
    state: &mut PublisherState,
    input: &PublisherInput,
    out: &mut Vec<SessionEvent>,
) -> Result<String, PublishError> {
    if let Some(id) = &state.assistant_message_id {
        return Ok(id.clone());
    }
    let id = crate::session::util::ascending();
    state.assistant_message_id = Some(id.clone());
    state.assistant_active = true;
    out.push(step_started(input, &id));
    Ok(id)
}

fn current_assistant(state: &PublisherState) -> Result<String, PublishError> {
    state
        .assistant_message_id
        .clone()
        .ok_or_else(|| PublishError::BeforeStart("tool event before assistant step start".into()))
}

fn flush_fragments(
    state: &mut PublisherState,
    input: &PublisherInput,
) -> Result<Vec<SessionEvent>, PublishError> {
    let mut out = Vec::new();
    for (id, text) in state.text.end_all()? {
        let assistant_message_id = current_assistant(state)?;
        out.push(SessionEvent::TextEnded {
            timestamp: timestamp_now(),
            session_id: input.session_id.clone(),
            assistant_message_id,
            text_id: id,
            text,
        });
    }
    for (id, text) in state.reasoning.end_all()? {
        let assistant_message_id = current_assistant(state)?;
        out.push(SessionEvent::ReasoningEnded {
            timestamp: timestamp_now(),
            session_id: input.session_id.clone(),
            assistant_message_id,
            reasoning_id: id,
            text,
            provider_metadata: None,
        });
    }
    for (id, text) in state.tool_input.end_all()? {
        let tool = state
            .tools
            .get(&id)
            .cloned()
            .ok_or_else(|| PublishError::UnknownTool(id.clone()))?;
        out.push(SessionEvent::ToolInputEnded {
            timestamp: timestamp_now(),
            session_id: input.session_id.clone(),
            assistant_message_id: tool.assistant_message_id,
            call_id: id,
            text,
        });
    }
    Ok(out)
}

fn fail_assistant(
    state: &mut PublisherState,
    input: &PublisherInput,
    message: &str,
) -> Result<Vec<SessionEvent>, PublishError> {
    let mut out = Vec::new();
    if state.assistant_failed {
        return Ok(out);
    }
    out.extend(flush_fragments(state, input)?);
    let assistant_message_id = start_assistant_transition(state, input, &mut out)?;
    state.assistant_active = false;
    state.assistant_failed = true;
    out.push(SessionEvent::StepFailed {
        timestamp: timestamp_now(),
        session_id: input.session_id.clone(),
        assistant_message_id,
        error: UnknownError::new(message),
    });
    Ok(out)
}

fn start_tool_input(
    state: &mut PublisherState,
    input: &PublisherInput,
    out: &mut Vec<SessionEvent>,
    id: &str,
    name: &str,
) -> Result<String, PublishError> {
    if state.tools.contains_key(id) {
        return Err(PublishError::Duplicate(format!("tool input start: {id}")));
    }
    let assistant_message_id = start_assistant_transition(state, input, out)?;
    state.tools.insert(
        id.to_string(),
        ToolEntry {
            assistant_message_id: assistant_message_id.clone(),
            name: name.to_string(),
            input_ended: false,
            called: false,
            settled: false,
            provider_executed: false,
            provider_metadata: None,
        },
    );
    state.tool_input.start(id)?;
    Ok(assistant_message_id)
}

fn end_tool_input(
    state: &mut PublisherState,
    input: &PublisherInput,
    out: &mut Vec<SessionEvent>,
    id: &str,
    name: &str,
) -> Result<String, PublishError> {
    let tool = state
        .tools
        .get(id)
        .ok_or_else(|| PublishError::BeforeStart(format!("tool input end: {id}")))?;
    if tool.name != *name {
        return Err(PublishError::NameChanged(id.to_string()));
    }
    if tool.input_ended {
        return Err(PublishError::Duplicate(format!("tool input end: {id}")));
    }
    let assistant_message_id = tool.assistant_message_id.clone();
    let text = state.tool_input.end(id)?;
    out.push(SessionEvent::ToolInputEnded {
        timestamp: timestamp_now(),
        session_id: input.session_id.clone(),
        assistant_message_id: assistant_message_id.clone(),
        call_id: id.to_string(),
        text,
    });
    if let Some(tool) = state.tools.get_mut(id) {
        tool.input_ended = true;
    }
    Ok(assistant_message_id)
}

fn handle(
    state: &mut PublisherState,
    input: &PublisherInput,
    event: &LLMEvent,
    output_paths: &[String],
) -> Result<Vec<SessionEvent>, PublishError> {
    let mut out = Vec::new();
    match event {
        LLMEvent::StepStart { .. } => {}
        LLMEvent::TextStart { id, .. } => {
            state.text.start(id)?;
            let assistant_message_id = start_assistant_transition(state, input, &mut out)?;
            out.push(SessionEvent::TextStarted {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                text_id: id.clone(),
            });
        }
        LLMEvent::TextDelta { id, text, .. } => {
            state.text.append(id, text)?;
            let assistant_message_id = current_assistant(state)?;
            out.push(SessionEvent::TextDelta {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                text_id: id.clone(),
                delta: text.clone(),
            });
        }
        LLMEvent::TextEnd { id, .. } => {
            let text = state.text.end(id)?;
            let assistant_message_id = current_assistant(state)?;
            out.push(SessionEvent::TextEnded {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                text_id: id.clone(),
                text,
            });
        }
        LLMEvent::ReasoningStart {
            id,
            provider_metadata,
        } => {
            state.reasoning.start(id)?;
            let assistant_message_id = start_assistant_transition(state, input, &mut out)?;
            out.push(SessionEvent::ReasoningStarted {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                reasoning_id: id.clone(),
                provider_metadata: provider_metadata.clone(),
            });
        }
        LLMEvent::ReasoningDelta { id, text, .. } => {
            state.reasoning.append(id, text)?;
            let assistant_message_id = current_assistant(state)?;
            out.push(SessionEvent::ReasoningDelta {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                reasoning_id: id.clone(),
                delta: text.clone(),
            });
        }
        LLMEvent::ReasoningEnd {
            id,
            provider_metadata,
        } => {
            let text = state.reasoning.end(id)?;
            let assistant_message_id = current_assistant(state)?;
            out.push(SessionEvent::ReasoningEnded {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                reasoning_id: id.clone(),
                text,
                provider_metadata: provider_metadata.clone(),
            });
        }
        LLMEvent::ToolInputStart { id, name, .. } => {
            let assistant_message_id = start_tool_input(state, input, &mut out, id, name)?;
            out.push(SessionEvent::ToolInputStarted {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                call_id: id.clone(),
                name: name.clone(),
            });
        }
        LLMEvent::ToolInputDelta { id, name, text } => {
            let tool = state
                .tools
                .get(id)
                .ok_or_else(|| PublishError::BeforeStart(format!("tool input delta: {id}")))?;
            if tool.name != *name {
                return Err(PublishError::NameChanged(id.clone()));
            }
            if tool.input_ended {
                return Err(PublishError::Duplicate(format!(
                    "tool input delta after end: {id}"
                )));
            }
            state.tool_input.append(id, text)?;
            let assistant_message_id = tool.assistant_message_id.clone();
            out.push(SessionEvent::ToolInputDelta {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id,
                call_id: id.clone(),
                delta: text.clone(),
            });
        }
        LLMEvent::ToolInputEnd { id, name, .. } => {
            let assistant_message_id = end_tool_input(state, input, &mut out, id, name)?;
            let _ = assistant_message_id;
        }
        LLMEvent::ToolCall {
            id,
            name,
            input: call_input,
            provider_executed,
            provider_metadata,
        } => {
            if !state.tools.contains_key(id) {
                start_tool_input(state, input, &mut out, id, name)?;
            }
            let tool = state
                .tools
                .get_mut(id)
                .ok_or_else(|| PublishError::UnknownTool(id.clone()))?;
            if !tool.input_ended {
                end_tool_input(state, input, &mut out, id, name)?;
            }
            let tool = state
                .tools
                .get_mut(id)
                .ok_or_else(|| PublishError::UnknownTool(id.clone()))?;
            if tool.name != *name {
                return Err(PublishError::NameChanged(id.clone()));
            }
            if tool.called {
                return Err(PublishError::Duplicate(format!("tool call: {id}")));
            }
            tool.called = true;
            tool.provider_executed = *provider_executed == Some(true);
            tool.provider_metadata = provider_metadata.clone();
            out.push(SessionEvent::ToolCalled {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id: tool.assistant_message_id.clone(),
                call_id: id.clone(),
                tool: name.clone(),
                input: record(call_input),
                provider: Provider::new(tool.provider_executed, tool.provider_metadata.clone()),
            });
        }
        LLMEvent::ToolResult {
            id,
            name,
            result,
            output,
            provider_executed,
            provider_metadata,
        } => {
            let tool = state
                .tools
                .get_mut(id)
                .ok_or_else(|| PublishError::ResultBeforeCall(id.clone()))?;
            if !tool.called {
                return Err(PublishError::ResultBeforeCall(id.clone()));
            }
            if tool.name != *name {
                return Err(PublishError::NameChanged(id.clone()));
            }
            if tool.settled {
                if result.is_error() {
                    return Ok(out);
                }
                return Err(PublishError::Duplicate(format!("tool result: {id}")));
            }
            tool.settled = true;
            let provider = Provider::new(
                *provider_executed == Some(true) || tool.provider_executed,
                provider_metadata.clone(),
            );
            match settled_output(output.clone(), result) {
                Err(error) => {
                    out.push(SessionEvent::ToolFailed {
                        timestamp: timestamp_now(),
                        session_id: input.session_id.clone(),
                        assistant_message_id: tool.assistant_message_id.clone(),
                        call_id: id.clone(),
                        error,
                        result: Some(result.clone()),
                        provider,
                    });
                }
                Ok(settled) => {
                    out.push(SessionEvent::ToolSuccess {
                        timestamp: timestamp_now(),
                        session_id: input.session_id.clone(),
                        assistant_message_id: tool.assistant_message_id.clone(),
                        call_id: id.clone(),
                        structured: settled.structured,
                        content: settled.content,
                        output_paths: if output_paths.is_empty() {
                            None
                        } else {
                            Some(output_paths.to_vec())
                        },
                        result: if provider.executed {
                            Some(result.clone())
                        } else {
                            None
                        },
                        provider,
                    });
                }
            }
        }
        LLMEvent::ToolError {
            id,
            name,
            message,
            provider_metadata,
        } => {
            let tool = state
                .tools
                .get_mut(id)
                .ok_or_else(|| PublishError::ErrorBeforeCall(id.clone()))?;
            if !tool.called {
                return Err(PublishError::ErrorBeforeCall(id.clone()));
            }
            if tool.name != *name {
                return Err(PublishError::NameChanged(id.clone()));
            }
            if tool.settled {
                return Err(PublishError::Duplicate(format!("tool error: {id}")));
            }
            tool.settled = true;
            out.push(SessionEvent::ToolFailed {
                timestamp: timestamp_now(),
                session_id: input.session_id.clone(),
                assistant_message_id: tool.assistant_message_id.clone(),
                call_id: id.clone(),
                error: UnknownError::new(message.clone()),
                result: None,
                provider: Provider::new(tool.provider_executed, provider_metadata.clone()),
            });
        }
        LLMEvent::StepFinish { reason, usage, .. } => {
            out.extend(flush_fragments(state, input)?);
            state.assistant_active = false;
            if state.step_settlement.is_some() {
                return Err(PublishError::DuplicateStepFinish);
            }
            state.step_settlement = Some(StepSettlement {
                finish: reason.clone(),
                tokens: tokens(usage.as_ref()),
                cost: cost(usage.as_ref(), input.cost.as_ref()),
            });
        }
        LLMEvent::Finish { .. } => {}
        LLMEvent::ProviderError(event) => {
            state.provider_failed = true;
            out.extend(fail_assistant(state, input, &event.message)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::services::EventBus;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Sink {
        events: Mutex<Vec<SessionEvent>>,
    }

    impl EventBus for Sink {
        fn publish(
            &self,
            event: SessionEvent,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            let events = &self.events;
            Box::pin(async move {
                events.lock().unwrap().push(event);
            })
        }
    }

    fn publisher() -> (Arc<LLMEventPublisher>, Arc<Sink>) {
        let sink = Arc::new(Sink::default());
        let publisher = Arc::new(LLMEventPublisher::new(
            sink.clone(),
            PublisherInput {
                session_id: "ses_1".into(),
                agent: "build".into(),
                model: ModelRef {
                    id: "gpt-4o".into(),
                    provider_id: "openai".into(),
                    variant: None,
                },
                cost: None,
                snapshot: Some("snap".into()),
            },
        ));
        (publisher, sink)
    }

    #[tokio::test]
    async fn text_stream_emits_expected_events() {
        let (publisher, sink) = publisher();
        publisher
            .publish(
                &LLMEvent::TextStart {
                    id: "t1".into(),
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        publisher
            .publish(
                &LLMEvent::TextDelta {
                    id: "t1".into(),
                    text: "hello".into(),
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        publisher
            .publish(
                &LLMEvent::TextDelta {
                    id: "t1".into(),
                    text: " world".into(),
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        publisher
            .publish(
                &LLMEvent::TextEnd {
                    id: "t1".into(),
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        let events = sink.events.lock().unwrap().clone();
        let types = events
            .iter()
            .map(|event| {
                serde_json::to_value(event)
                    .unwrap()
                    .get("type")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            types,
            vec![
                "session.next.step.started",
                "session.next.text.started",
                "session.next.text.delta",
                "session.next.text.delta",
                "session.next.text.ended",
            ]
        );
        // text-ends join deltas
        let ended = events
            .iter()
            .find(|event| matches!(event, SessionEvent::TextEnded { .. }))
            .unwrap();
        let value = serde_json::to_value(ended).unwrap();
        assert_eq!(
            value.get("text").unwrap(),
            &serde_json::json!("hello world")
        );
    }

    #[tokio::test]
    async fn tool_call_then_result_success() {
        let (publisher, sink) = publisher();
        publisher
            .publish(
                &LLMEvent::ToolCall {
                    id: "call_1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({ "command": "ls" }),
                    provider_executed: None,
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        publisher
            .publish(
                &LLMEvent::ToolResult {
                    id: "call_1".into(),
                    name: "bash".into(),
                    result: ToolResultValue::Text {
                        value: serde_json::json!("a\nb"),
                    },
                    output: None,
                    provider_executed: None,
                    provider_metadata: None,
                },
                &["out1".to_string()],
            )
            .await
            .unwrap();
        let events = sink.events.lock().unwrap().clone();
        assert!(events
            .iter()
            .any(|event| matches!(event, SessionEvent::ToolCalled { .. })));
        let success = events
            .iter()
            .find(|event| matches!(event, SessionEvent::ToolSuccess { .. }))
            .unwrap();
        let value = serde_json::to_value(success).unwrap();
        assert_eq!(
            value.get("outputPaths").unwrap(),
            &serde_json::json!(["out1"])
        );
        assert!(value.get("result").is_none());
    }

    #[tokio::test]
    async fn tool_call_then_result_error_marks_failed() {
        let (publisher, sink) = publisher();
        publisher
            .publish(
                &LLMEvent::ToolCall {
                    id: "call_2".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                    provider_executed: None,
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        publisher
            .publish(
                &LLMEvent::ToolResult {
                    id: "call_2".into(),
                    name: "bash".into(),
                    result: ToolResultValue::Error {
                        value: serde_json::json!("boom"),
                    },
                    output: None,
                    provider_executed: None,
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        let events = sink.events.lock().unwrap().clone();
        let failed = events
            .iter()
            .find(|event| matches!(event, SessionEvent::ToolFailed { .. }))
            .unwrap();
        let value = serde_json::to_value(failed).unwrap();
        assert_eq!(
            value.get("error").unwrap().get("message").unwrap(),
            &serde_json::json!("boom")
        );
        assert!(value.get("result").is_some());
    }

    #[tokio::test]
    async fn step_finish_records_settlement() {
        let (publisher, _sink) = publisher();
        publisher
            .publish(
                &LLMEvent::StepFinish {
                    index: 0.0,
                    reason: "stop".into(),
                    usage: Some(Usage {
                        non_cached_input_tokens: Some(10.0),
                        output_tokens: Some(5.0),
                        reasoning_tokens: Some(2.0),
                        ..Default::default()
                    }),
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        let settlement = publisher.step_settlement().unwrap();
        assert_eq!(settlement.finish, "stop");
        assert_eq!(settlement.tokens.input, 10.0);
        assert_eq!(settlement.tokens.output, 3.0);
        assert_eq!(settlement.tokens.reasoning, 2.0);
    }

    #[tokio::test]
    async fn step_finish_calculates_catalog_cost() {
        let sink = Arc::new(Sink::default());
        let publisher = LLMEventPublisher::new(
            sink,
            PublisherInput {
                session_id: "ses_priced".into(),
                agent: "build".into(),
                model: ModelRef {
                    id: "priced".into(),
                    provider_id: "test".into(),
                    variant: None,
                },
                cost: Some(ModelCost {
                    input: 2.0,
                    output: 4.0,
                    cache_read: 0.5,
                    cache_write: 1.0,
                }),
                snapshot: None,
            },
        );
        publisher
            .publish(
                &LLMEvent::StepFinish {
                    index: 0.0,
                    reason: "stop".into(),
                    usage: Some(Usage {
                        non_cached_input_tokens: Some(100.0),
                        output_tokens: Some(8.0),
                        reasoning_tokens: Some(2.0),
                        cache_read_input_tokens: Some(10.0),
                        ..Default::default()
                    }),
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        let settlement = publisher.step_settlement().unwrap();
        assert!((settlement.cost - 0.000229).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn provider_error_fails_assistant() {
        let (publisher, sink) = publisher();
        publisher
            .publish(
                &LLMEvent::ProviderError(crate::llm::event::ProviderErrorEvent {
                    kind: crate::llm::event::ProviderErrorKind::ProviderError,
                    message: "nope".into(),
                    classification: None,
                    retryable: None,
                    provider_metadata: None,
                }),
                &[],
            )
            .await
            .unwrap();
        assert!(publisher.has_provider_error());
        let events = sink.events.lock().unwrap().clone();
        assert!(events
            .iter()
            .any(|event| matches!(event, SessionEvent::StepFailed { .. })));
    }

    #[tokio::test]
    async fn fail_unsettled_tools_hosted_only_skips_local() {
        let (publisher, sink) = publisher();
        publisher
            .publish(
                &LLMEvent::ToolCall {
                    id: "call_local".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                    provider_executed: None,
                    provider_metadata: None,
                },
                &[],
            )
            .await
            .unwrap();
        publisher
            .fail_unsettled_tools("interrupted", true)
            .await
            .unwrap();
        let events = sink.events.lock().unwrap().clone();
        assert!(!events
            .iter()
            .any(|event| matches!(event, SessionEvent::ToolFailed { .. })));

        publisher
            .fail_unsettled_tools("interrupted", false)
            .await
            .unwrap();
        let events = sink.events.lock().unwrap().clone();
        assert!(events
            .iter()
            .any(|event| matches!(event, SessionEvent::ToolFailed { .. })));
    }
}
