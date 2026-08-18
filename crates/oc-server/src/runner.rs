//! Production adapters for the durable session runner.
//!
//! The runner crate owns the orchestration contracts while `oc-server` owns
//! the HTTP projection and `oc-llm` owns provider routes.  This module is the
//! explicit bridge between those layers.  It is deliberately fail-closed for
//! local tools until the permission/tool registry is connected: a provider
//! cannot cause an unreviewed filesystem or process side effect merely because
//! a session was started through the server.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use oc_session::store::{SessionDb as _, SqliteSessionDb};
use oc_session_runner::llm::error::{LLMError, LLMErrorReason, ReasonMessage};
use oc_session_runner::llm::message::{
    ContentPart as RunnerContentPart, GenerationOptions as RunnerGenerationOptions,
    HttpOptions as RunnerHttpOptions, LLMRequest, Message as RunnerMessage,
    Model as RunnerLlmModel, ModelCost as RunnerModelCost, ModelLimits as RunnerModelLimits,
    SystemPart as RunnerSystemPart, ToolChoice as RunnerToolChoice,
};
use oc_session_runner::runner::llm::{RunnerDeps, SessionRunnerService};
use oc_session_runner::runner::model::{ModelError, ModelNotSelectedError, SessionRunnerModel};
use oc_session_runner::session::event::SessionEvent;
use oc_session_runner::session::message::{
    AgentAttachment, Assistant, AssistantContent, AssistantContentKind, AssistantText, Compaction,
    CompactionReason, FileAttachment, MessageKind, MessageTime, SessionMessage, System,
    Tokens as RunnerMessageTokens, User,
};
use oc_session_runner::session::schema::{
    Location as RunnerLocation, LocationRef as RunnerLocationRef, ModelRef as RunnerModelRef,
    SessionID, SessionInfo as RunnerSessionInfo,
};
use oc_session_runner::session::services::{
    AgentInfo, AgentSelection, Agents, CompactionInput, Delivery, EventBus, HistoryEntry,
    LlmClient, LlmEventStream, LocationService, PreparedContext, ReferenceGuidance,
    SessionCompaction, SessionContextEpoch, SessionHistory, SessionInput, SessionStore,
    SkillGuidance, Snapshots, SystemContext, SystemContextRegistry, ToolMaterialization,
    ToolRegistry, ToolSettle, ToolSettlementError,
};

use crate::event::{event_id, session_message_id, Event};
use crate::schema::{ModelRef, SessionInfo};
use crate::state::{now_millis, AppState, SessionRecord};

/// Schedule one explicit provider turn through the durable runner. Prompts
/// admitted while a turn is active are coalesced into a follow-up pass.
pub fn schedule_session_run(state: AppState, session_id: String) {
    tokio::spawn(async move {
        let Some(mut token) = state.acquire_session_run(&session_id).await else {
            return;
        };
        loop {
            run_session_with_token(state.clone(), session_id.clone(), token).await;
            let Some(next) = state.finish_session_run(&session_id).await else {
                break;
            };
            token = next;
        }
    });
}

/// Run one provider pass. The scheduler above owns serialization across
/// prompts; keeping this function separate makes cancellation and runner
/// integration tests deterministic.
pub async fn run_session(state: AppState, session_id: String) {
    run_session_with_token(state, session_id, CancellationToken::new()).await;
}

async fn run_session_with_token(state: AppState, session_id: String, token: CancellationToken) {
    emit_status(&state, &session_id, "busy");

    let turns = Arc::new(Mutex::new(HashMap::new()));
    let deps = RunnerDeps {
        events: Arc::new(ServerEventBus {
            state: state.clone(),
            turns: turns.clone(),
        }),
        llm: Arc::new(ServerLlm {
            state: state.clone(),
        }),
        agents: Arc::new(ServerAgents {
            state: state.clone(),
        }),
        tools: Arc::new(ServerTools {
            state: state.clone(),
        }),
        models: Arc::new(ServerModel {
            state: state.clone(),
        }),
        store: Arc::new(ServerStore {
            state: state.clone(),
        }),
        location: Arc::new(ServerLocation {
            state: state.clone(),
            session_id: session_id.clone(),
        }),
        system_context: Arc::new(EmptySystemContext),
        skill_guidance: Arc::new(ServerSkillGuidance {
            state: state.clone(),
            session_id: session_id.clone(),
        }),
        reference_guidance: Arc::new(EmptyReferenceGuidance),
        snapshots: Arc::new(EmptySnapshots),
        input: Arc::new(ServerInput {
            state: state.clone(),
        }),
        history: Arc::new(ServerHistory {
            state: state.clone(),
        }),
        context_epoch: Arc::new(ServerContextEpoch {
            state: state.clone(),
        }),
        compaction: Arc::new(ServerCompaction {
            state: state.clone(),
        }),
    };
    let service = SessionRunnerService::new(deps);
    let result = service.run(&session_id, true, &token).await;

    if let Err(error) = result {
        if let Ok(mut stores) = state.stores.try_write() {
            if let Some(record) = stores.sessions.get_mut(&session_id) {
                record.active = false;
            }
        }
        state.emit_event(Event {
            id: event_id(),
            metadata: None,
            r#type: "session.error".into(),
            durable: None,
            location: None,
            data: json!({
                "sessionID": session_id,
                "error": { "name": "SessionRunnerError", "data": { "message": error.to_string() } }
            }),
        });
    }

    emit_status(&state, &session_id, "idle");
}

fn emit_status(state: &AppState, session_id: &str, status: &str) {
    state.emit_event(Event {
        id: event_id(),
        metadata: None,
        r#type: "session.status".into(),
        durable: None,
        location: None,
        data: json!({ "sessionID": session_id, "status": { "type": status } }),
    });
}

#[derive(Clone)]
struct ActiveTurn {
    assistant_message_id: String,
    agent: String,
    model: RunnerModelRef,
    text_id: Option<String>,
    text: String,
    reasoning_id: Option<String>,
    reasoning: String,
    tools: HashMap<String, Value>,
}

#[derive(Clone)]
struct ServerEventBus {
    state: AppState,
    turns: Arc<Mutex<HashMap<String, ActiveTurn>>>,
}

impl EventBus for ServerEventBus {
    fn publish(&self, event: SessionEvent) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let state = self.state.clone();
        let turns = self.turns.clone();
        Box::pin(async move {
            let wire = serde_json::to_value(&event).unwrap_or_else(|_| json!({}));
            let event_type = wire
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("session.next.unknown")
                .to_string();
            state.emit_event(Event {
                id: event_id(),
                metadata: None,
                r#type: event_type,
                durable: None,
                location: None,
                data: wire,
            });

            match event {
                SessionEvent::StepStarted {
                    session_id,
                    assistant_message_id,
                    agent,
                    model,
                    ..
                } => {
                    turns.lock().await.insert(
                        session_id.clone(),
                        ActiveTurn {
                            assistant_message_id,
                            agent,
                            model,
                            text_id: None,
                            text: String::new(),
                            reasoning_id: None,
                            reasoning: String::new(),
                            tools: HashMap::new(),
                        },
                    );
                    set_active(&state, &session_id, true).await;
                }
                SessionEvent::TextStarted {
                    session_id,
                    text_id,
                    ..
                } => {
                    if let Some(turn) = turns.lock().await.get_mut(&session_id) {
                        turn.text_id = Some(text_id);
                    }
                }
                SessionEvent::ReasoningStarted {
                    session_id,
                    reasoning_id,
                    ..
                } => {
                    if let Some(turn) = turns.lock().await.get_mut(&session_id) {
                        turn.reasoning_id = Some(reasoning_id);
                        turn.reasoning.clear();
                    }
                }
                SessionEvent::ReasoningDelta {
                    session_id, delta, ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        turn.reasoning.push_str(&delta);
                        reasoning_part(turn, false)
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ReasoningEnded {
                    session_id, text, ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        if turn.reasoning.is_empty() {
                            turn.reasoning = text;
                        }
                        reasoning_part(turn, true)
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ToolInputStarted {
                    session_id,
                    call_id,
                    name,
                    ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        let part = tool_part(&turn.assistant_message_id, &call_id, &name);
                        turn.tools.insert(call_id, part.clone());
                        part
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ToolInputDelta {
                    session_id,
                    call_id,
                    delta,
                    ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        let Some(tool) = turn.tools.get_mut(&call_id) else {
                            return;
                        };
                        let raw = tool["state"]["input"].as_str().unwrap_or_default();
                        tool["state"]["input"] = json!(format!("{raw}{delta}"));
                        tool.clone()
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ToolInputEnded {
                    session_id,
                    call_id,
                    text,
                    ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        let Some(tool) = turn.tools.get_mut(&call_id) else {
                            return;
                        };
                        tool["state"]["input"] = json!(text);
                        tool.clone()
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::TextDelta {
                    session_id, delta, ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        turn.text.push_str(&delta);
                        text_part(turn, false)
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::TextEnded {
                    session_id, text, ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        if turn.text.is_empty() {
                            turn.text = text;
                        }
                        text_part(turn, true)
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ToolCalled {
                    session_id,
                    call_id,
                    tool,
                    input,
                    provider,
                    ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        let assistant_message_id = turn.assistant_message_id.clone();
                        let tool_part = turn
                            .tools
                            .entry(call_id.clone())
                            .or_insert_with(|| tool_part(&assistant_message_id, &call_id, &tool));
                        tool_part["name"] = json!(tool);
                        tool_part["provider"] = json!(provider);
                        tool_part["state"] = json!({
                            "status": "running",
                            "input": input,
                            "structured": {},
                            "content": []
                        });
                        tool_part["time"]["ran"] = json!(now_millis().to_string());
                        tool_part.clone()
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ToolProgress {
                    session_id,
                    call_id,
                    structured,
                    content,
                    ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        let Some(tool) = turn.tools.get_mut(&call_id) else {
                            return;
                        };
                        tool["state"]["structured"] = json!(structured);
                        tool["state"]["content"] = json!(content);
                        tool.clone()
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ToolSuccess {
                    session_id,
                    call_id,
                    structured,
                    content,
                    output_paths,
                    result,
                    provider,
                    ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        let Some(tool) = turn.tools.get_mut(&call_id) else {
                            return;
                        };
                        tool["provider"] = json!(provider);
                        tool["state"] = json!({
                            "status": "completed",
                            "input": tool["state"].get("input").cloned().unwrap_or_else(|| json!({})),
                            "structured": structured,
                            "content": content,
                            "outputPaths": output_paths,
                            "result": result,
                        });
                        tool["time"]["ran"] = json!(now_millis().to_string());
                        tool["time"]["completed"] = json!(now_millis().to_string());
                        tool.clone()
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::ToolFailed {
                    session_id,
                    call_id,
                    error,
                    result,
                    provider,
                    ..
                } => {
                    let part = {
                        let mut guard = turns.lock().await;
                        let Some(turn) = guard.get_mut(&session_id) else {
                            return;
                        };
                        let Some(tool) = turn.tools.get_mut(&call_id) else {
                            return;
                        };
                        tool["provider"] = json!(provider);
                        let input = tool["state"]
                            .get("input")
                            .cloned()
                            .unwrap_or_else(|| json!({}));
                        tool["state"] = json!({
                            "status": "error",
                            "input": input,
                            "structured": {},
                            "content": [],
                            "error": error,
                            "result": result,
                        });
                        tool["time"]["completed"] = json!(now_millis().to_string());
                        tool.clone()
                    };
                    emit_part(&state, &session_id, &part);
                }
                SessionEvent::StepEnded {
                    session_id,
                    assistant_message_id,
                    finish,
                    cost,
                    tokens,
                    ..
                } => {
                    let turn = turns.lock().await.remove(&session_id);
                    let (agent, model, reasoning, text, tools) = turn
                        .map(|turn| {
                            (
                                turn.agent,
                                turn.model,
                                turn.reasoning,
                                turn.text,
                                turn.tools.into_values().collect::<Vec<_>>(),
                            )
                        })
                        .unwrap_or_else(|| {
                            (
                                "build".into(),
                                RunnerModelRef {
                                    id: String::new(),
                                    provider_id: String::new(),
                                    variant: None,
                                },
                                String::new(),
                                String::new(),
                                Vec::new(),
                            )
                        });
                    let created = now_millis();
                    let mut content = Vec::new();
                    if !reasoning.is_empty() {
                        content.push(json!({
                            "type": "reasoning",
                            "id": format!("{assistant_message_id}_reasoning"),
                            "text": reasoning,
                        }));
                    }
                    if !text.is_empty() {
                        content.push(json!({
                            "type": "text",
                            "id": format!("{assistant_message_id}_text"),
                            "text": text,
                        }));
                    }
                    content.extend(tools);
                    let assistant = json!({
                        "id": assistant_message_id,
                        "type": "assistant",
                        "role": "assistant",
                        "agent": agent,
                        "modelID": model.id,
                        "providerID": model.provider_id,
                        "reasoning": reasoning,
                        "text": text,
                        "content": content,
                        "finish": finish,
                        "cost": cost,
                        "tokens": {
                            "input": tokens.input,
                            "output": tokens.output,
                            "reasoning": tokens.reasoning,
                            "cache": { "read": tokens.cache.read, "write": tokens.cache.write }
                        },
                        "time": { "created": created, "completed": created }
                    });
                    if let Some(info) =
                        append_assistant(&state, &session_id, assistant.clone()).await
                    {
                        emit_usage_updated(&state, &session_id, &info);
                    }
                    state.emit_event(Event {
                        id: event_id(),
                        metadata: None,
                        r#type: "message.updated".into(),
                        durable: None,
                        location: None,
                        data: json!({ "sessionID": session_id, "info": assistant }),
                    });
                }
                SessionEvent::StepFailed {
                    session_id,
                    assistant_message_id,
                    error,
                    ..
                } => {
                    let turn = turns.lock().await.remove(&session_id);
                    let assistant = failed_assistant(turn, &assistant_message_id, &error);
                    append_assistant(&state, &session_id, assistant.clone()).await;
                    state.emit_event(Event {
                        id: event_id(),
                        metadata: None,
                        r#type: "message.updated".into(),
                        durable: None,
                        location: None,
                        data: json!({ "sessionID": session_id, "info": assistant }),
                    });
                    state.emit_event(Event {
                        id: event_id(),
                        metadata: None,
                        r#type: "session.error".into(),
                        durable: None,
                        location: None,
                        data: json!({
                            "sessionID": session_id,
                            "error": {
                                "name": "ProviderError",
                                "data": { "message": error.message, "assistantMessageID": assistant_message_id }
                            }
                        }),
                    });
                }
                SessionEvent::Retried {
                    session_id,
                    attempt,
                    error,
                    ..
                } => {
                    let retry_message = error.message.clone();
                    let retry_at = now_millis();
                    let assistant_message_id = turns
                        .lock()
                        .await
                        .get(&session_id)
                        .map(|turn| turn.assistant_message_id.clone());
                    let retry_part = turns.lock().await.get(&session_id).map(|turn| {
                        json!({
                            "id": format!("{}_retry_{attempt}", turn.assistant_message_id),
                            "messageID": turn.assistant_message_id,
                            "type": "retry",
                            "attempt": attempt,
                            "error": error,
                            "time": { "start": retry_at }
                        })
                    });
                    if let Some(part) = retry_part {
                        emit_part(&state, &session_id, &part);
                    }
                    if let Some(assistant_message_id) = assistant_message_id {
                        state.emit_event(Event {
                            id: event_id(),
                            metadata: None,
                            r#type: "session.retry.scheduled".into(),
                            durable: None,
                            location: None,
                            data: json!({
                                "sessionID": session_id,
                                "assistantMessageID": assistant_message_id,
                                "attempt": attempt,
                                "at": retry_at,
                                "error": error,
                            }),
                        });
                    }
                    state.emit_event(Event {
                        id: event_id(),
                        metadata: None,
                        r#type: "session.status".into(),
                        durable: None,
                        location: None,
                        data: json!({
                            "sessionID": session_id,
                            "status": {
                                "type": "retry",
                                "attempt": attempt,
                                "message": retry_message,
                            }
                        }),
                    });
                }
                _ => {}
            }
        })
    }
}

fn tool_part(assistant_message_id: &str, call_id: &str, name: &str) -> Value {
    json!({
        "id": call_id,
        "messageID": assistant_message_id,
        "type": "tool",
        "name": name,
        "state": { "status": "pending", "input": "" },
        "time": { "created": now_millis().to_string() }
    })
}

fn failed_assistant(
    turn: Option<ActiveTurn>,
    assistant_message_id: &str,
    error: &oc_session_runner::session::message::UnknownError,
) -> Value {
    let (agent, model, reasoning, text, tools) = turn
        .map(|turn| {
            (
                turn.agent,
                turn.model,
                turn.reasoning,
                turn.text,
                turn.tools.into_values().collect::<Vec<_>>(),
            )
        })
        .unwrap_or_else(|| {
            (
                "build".into(),
                RunnerModelRef {
                    id: String::new(),
                    provider_id: String::new(),
                    variant: None,
                },
                String::new(),
                String::new(),
                Vec::new(),
            )
        });
    let created = now_millis();
    let mut content = Vec::new();
    if !reasoning.is_empty() {
        content.push(json!({
            "type": "reasoning",
            "id": format!("{assistant_message_id}_reasoning"),
            "text": reasoning,
        }));
    }
    if !text.is_empty() {
        content.push(json!({
            "type": "text",
            "id": format!("{assistant_message_id}_text"),
            "text": text,
        }));
    }
    content.extend(tools);
    json!({
        "id": assistant_message_id,
        "type": "assistant",
        "role": "assistant",
        "agent": agent,
        "modelID": model.id,
        "providerID": model.provider_id,
        "reasoning": reasoning,
        "text": text,
        "content": content,
        "finish": "error",
        "error": error,
        "cost": 0,
        "tokens": {
            "input": 0,
            "output": 0,
            "reasoning": 0,
            "cache": { "read": 0, "write": 0 }
        },
        "time": { "created": created, "completed": created }
    })
}

fn text_part(turn: &ActiveTurn, complete: bool) -> Value {
    let start = now_millis();
    let time = if complete {
        json!({ "start": start, "end": now_millis() })
    } else {
        json!({ "start": start })
    };
    json!({
        "id": turn.text_id.clone().unwrap_or_else(|| format!("{}_text", turn.assistant_message_id)),
        "messageID": turn.assistant_message_id,
        "type": "text",
        "text": turn.text,
        "time": time
    })
}

fn reasoning_part(turn: &ActiveTurn, complete: bool) -> Value {
    let start = now_millis();
    let time = if complete {
        json!({ "start": start, "end": now_millis() })
    } else {
        json!({ "start": start })
    };
    json!({
        "id": turn
            .reasoning_id
            .clone()
            .unwrap_or_else(|| format!("{}_reasoning", turn.assistant_message_id)),
        "messageID": turn.assistant_message_id,
        "type": "reasoning",
        "text": turn.reasoning,
        "time": time
    })
}

fn emit_part(state: &AppState, session_id: &str, part: &Value) {
    if let Some(message_id) = part.get("messageID").and_then(Value::as_str) {
        state.persist_part(session_id, message_id, part);
    }
    state.emit_event(Event {
        id: event_id(),
        metadata: None,
        r#type: "message.part.updated".into(),
        durable: None,
        location: None,
        data: json!({ "sessionID": session_id, "part": part }),
    });
}

async fn set_active(state: &AppState, session_id: &str, active: bool) {
    let mut stores = state.stores.write().await;
    if let Some(record) = stores.sessions.get_mut(session_id) {
        record.active = active;
    }
}

async fn append_assistant(
    state: &AppState,
    session_id: &str,
    assistant: Value,
) -> Option<SessionInfo> {
    let info = {
        let mut stores = state.stores.write().await;
        let Some(record) = stores.sessions.get_mut(session_id) else {
            return None;
        };
        record.messages.push(assistant.clone());
        record.active = false;
        record.info.cost += assistant.get("cost").and_then(Value::as_f64).unwrap_or(0.0);
        if let Some(tokens) = assistant.get("tokens") {
            record.info.tokens.input += tokens.get("input").and_then(Value::as_f64).unwrap_or(0.0);
            record.info.tokens.output +=
                tokens.get("output").and_then(Value::as_f64).unwrap_or(0.0);
            record.info.tokens.reasoning += tokens
                .get("reasoning")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            if let Some(cache) = tokens.get("cache") {
                record.info.tokens.cache.read +=
                    cache.get("read").and_then(Value::as_f64).unwrap_or(0.0);
                record.info.tokens.cache.write +=
                    cache.get("write").and_then(Value::as_f64).unwrap_or(0.0);
            }
        }
        record.info.time.updated = now_millis();
        record.info.clone()
    };
    state.persist_message(session_id, &assistant);
    state.persist_session(&info);
    Some(info)
}

fn emit_usage_updated(state: &AppState, session_id: &str, info: &SessionInfo) {
    state.emit_event(Event {
        id: event_id(),
        metadata: None,
        r#type: "session.usage.updated".into(),
        durable: None,
        location: None,
        data: json!({
            "sessionID": session_id,
            "cost": info.cost,
            "tokens": {
                "input": info.tokens.input,
                "output": info.tokens.output,
                "reasoning": info.tokens.reasoning,
                "cache": {
                    "read": info.tokens.cache.read,
                    "write": info.tokens.cache.write,
                },
            },
        }),
    });
}

#[derive(Clone)]
struct ServerStore {
    state: AppState,
}

impl SessionStore for ServerStore {
    fn get(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = Option<RunnerSessionInfo>> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let stores = state.stores.read().await;
            stores
                .sessions
                .get(&session_id)
                .map(|record| runner_session_info(&record.info))
        })
    }

    fn context(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = Vec<SessionMessage>> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let stores = state.stores.read().await;
            stores
                .sessions
                .get(&session_id)
                .map(|record| {
                    record
                        .messages
                        .iter()
                        .filter_map(server_message)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    }
}

#[derive(Clone)]
struct ServerHistory {
    state: AppState,
}

/// Persist a compacted conversation checkpoint while retaining the original
/// message log for UI/history consumers. Runner history starts at the latest
/// checkpoint, so this also provides the durable input to automatic
/// compaction recovery.
pub(crate) async fn compact_session(
    state: &AppState,
    session_id: &str,
    reason: CompactionReason,
) -> bool {
    compact_session_with_summary(state, session_id, reason, None).await
}

/// Generate a provider-backed summary for an explicit summarize request, then
/// persist the same durable checkpoint used by automatic compaction. When no
/// model is available, the deterministic checkpoint remains the safe fallback
/// used by legacy callers.
pub(crate) async fn summarize_and_compact_session(
    state: &AppState,
    session_id: &str,
    reason: CompactionReason,
    requested_model: Option<ModelRef>,
) -> Result<bool, String> {
    let (info, messages) = {
        let stores = state.stores.read().await;
        let record = stores
            .sessions
            .get(session_id)
            .ok_or_else(|| format!("session not found: {session_id}"))?;
        (record.info.clone(), record.messages.clone())
    };
    let model = requested_model.or(info.model);
    let Some(model) = model else {
        return Ok(compact_session_with_summary(state, session_id, reason, None).await);
    };
    let transcript = compact_message_lines(&messages);
    if transcript.trim().is_empty() {
        return Ok(compact_session_with_summary(state, session_id, reason, None).await);
    }
    let prompt = oc_session::compaction_core::build_prompt(None, &[transcript.as_str()]);
    let request = LLMRequest {
        id: Some(format!("summary_{session_id}")),
        model: RunnerLlmModel::make(model.id, model.provider_id),
        system: vec![RunnerSystemPart::make(
            "You are OpenCode's conversation compactor. Return only a concise durable summary of the conversation, preserving goals, decisions, constraints, files, errors, and unfinished work.",
        )],
        messages: vec![RunnerMessage::user(vec![RunnerContentPart::text(prompt)])],
        tools: Vec::new(),
        tool_choice: None,
        generation: Some(RunnerGenerationOptions {
            temperature: Some(0.2),
            top_p: None,
            top_k: None,
            max_tokens: Some(oc_session::compaction_core::SUMMARY_OUTPUT_TOKENS),
        }),
        provider_options: None,
        http: None,
    };
    let mut events = ServerLlm {
        state: state.clone(),
    }
    .stream(request)
    .await
    .map_err(|error| error.to_string())?;
    let mut summary = String::new();
    while let Some(event) = events.next().await {
        let event = event.map_err(|error| error.to_string())?;
        if let oc_session_runner::llm::LLMEvent::TextDelta { text, .. } = event {
            summary.push_str(&text);
        }
    }
    let summary = summary.trim().to_string();
    if summary.is_empty() {
        return Err("summary provider returned no text".into());
    }
    Ok(compact_session_with_summary(state, session_id, reason, Some(summary)).await)
}

async fn compact_session_with_summary(
    state: &AppState,
    session_id: &str,
    reason: CompactionReason,
    summary_override: Option<String>,
) -> bool {
    let message_id = session_message_id();
    let (info, checkpoint, pruned_messages, pruned_parts) = {
        let mut stores = state.stores.write().await;
        let config = stores.config.clone();
        let Some(record) = stores.sessions.get_mut(session_id) else {
            return false;
        };
        let prune_candidates = legacy_prune_candidates(&record.messages);
        let pruned_parts = mark_legacy_pruned_parts(
            &mut record.messages,
            &prune_candidates,
            &now_millis().to_string(),
        );
        let mut pruned_message_indices = pruned_parts
            .iter()
            .map(|part| part.message_index)
            .collect::<Vec<_>>();
        pruned_message_indices.sort_unstable();
        pruned_message_indices.dedup();
        let pruned_messages = pruned_message_indices
            .into_iter()
            .filter_map(|index| {
                record
                    .messages
                    .get(index)
                    .and_then(|message| message.get("id").and_then(Value::as_str))
                    .map(|id| (id.to_string(), record.messages[index].clone()))
            })
            .collect::<Vec<_>>();
        let recent_count = compaction_recent_count(&record.messages, &config);
        let summary_end = record.messages.len().saturating_sub(recent_count);
        let summary = summary_override
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| compact_message_lines(&record.messages[..summary_end]));
        let recent =
            serde_json::to_string(&record.messages[summary_end..]).unwrap_or_else(|_| "[]".into());
        let summary = if summary.is_empty() {
            "No earlier conversation was available; preserve the recent context below.".to_string()
        } else {
            summary
        };
        let recent = trim_chars(&recent, 48_000);
        let checkpoint = json!({
            "id": message_id,
            "type": "compaction",
            "reason": match reason {
                CompactionReason::Auto => "auto",
                CompactionReason::Manual => "manual",
            },
            "summary": summary,
            "recent": recent,
            "time": { "created": now_millis() },
        });
        record.messages.push(checkpoint.clone());
        record.info.time.updated = now_millis();
        (
            record.info.clone(),
            checkpoint,
            pruned_messages,
            pruned_parts,
        )
    };

    for (message_id, message) in pruned_messages {
        state.persist_message(session_id, &message);
        for part in pruned_parts
            .iter()
            .filter(|part| part.message_id == message_id)
        {
            state.persist_part(session_id, &message_id, &part.part);
        }
    }
    state.persist_message(session_id, &checkpoint);
    state.persist_session(&info);
    state.emit_event(Event {
        id: event_id(),
        metadata: None,
        r#type: "session.compacted".into(),
        durable: None,
        location: None,
        data: json!({ "sessionID": session_id }),
    });
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LegacyPruneCandidate {
    message_index: usize,
    part_id: String,
}

#[derive(Debug, Clone)]
struct LegacyPrunedPart {
    message_index: usize,
    message_id: String,
    part: Value,
}

/// Estimate only the completed tool result fields that the legacy JSON
/// schema exposes. This is intentionally conservative and uses the same
/// four-bytes-per-token approximation as the surrounding server compaction
/// path; it is only used for the typed helper's 20k/40k pruning thresholds.
fn legacy_tool_output_tokens(part: &Value) -> u64 {
    let Some(state) = part.get("state") else {
        return 0;
    };
    let output = json!({
        "attachments": state.get("attachments"),
        "content": state.get("content"),
        "outputPaths": state.get("outputPaths"),
        "result": state.get("result"),
        "structured": state.get("structured"),
    });
    serde_json::to_string(&output)
        .map(|value| {
            value
                .len()
                .saturating_add(COMPACTION_TOKEN_BYTES - 1)
                .checked_div(COMPACTION_TOKEN_BYTES)
                .unwrap_or(usize::MAX)
                .max(1) as u64
        })
        .unwrap_or(1)
}

/// Mirror `oc_session::compaction::prune_candidates` over the server's
/// legacy JSON messages. The legacy/session-runner wire schema calls the
/// persisted marker `time.pruned`; `time.compacted` is accepted when reading
/// older typed-v1 data, but no new field is invented here.
fn legacy_prune_candidates(messages: &[Value]) -> Vec<LegacyPruneCandidate> {
    let mut total = 0u64;
    let mut pruned = 0u64;
    let mut candidates = Vec::new();
    let mut turn_count = 0usize;

    'outer: for (message_index, message) in messages.iter().enumerate().rev() {
        let kind = message
            .get("type")
            .or_else(|| message.get("role"))
            .and_then(Value::as_str);
        if kind == Some("user") {
            turn_count += 1;
        }
        if turn_count < 2 {
            continue;
        }
        if kind == Some("assistant")
            && message.get("summary").and_then(Value::as_bool) == Some(true)
        {
            break;
        }

        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in parts.iter().rev() {
            if part.get("type").and_then(Value::as_str) != Some("tool")
                || part
                    .get("state")
                    .and_then(|state| state.get("status"))
                    .and_then(Value::as_str)
                    != Some("completed")
            {
                continue;
            }
            let Some(tool_name) = part.get("name").and_then(Value::as_str) else {
                continue;
            };
            if oc_session::compaction::PRUNE_PROTECTED_TOOLS.contains(&tool_name) {
                continue;
            }
            let already_pruned = part
                .get("time")
                .and_then(|time| time.get("pruned").or_else(|| time.get("compacted")))
                .is_some();
            if already_pruned {
                break 'outer;
            }

            let estimate = legacy_tool_output_tokens(part);
            total = total.saturating_add(estimate);
            if total <= oc_session::compaction::PRUNE_PROTECT {
                continue;
            }
            pruned = pruned.saturating_add(estimate);
            if let Some(part_id) = part.get("id").and_then(Value::as_str) {
                candidates.push(LegacyPruneCandidate {
                    message_index,
                    part_id: part_id.to_string(),
                });
            }
        }
    }

    if pruned > oc_session::compaction::PRUNE_MINIMUM {
        candidates
    } else {
        Vec::new()
    }
}

fn mark_legacy_pruned_parts(
    messages: &mut [Value],
    candidates: &[LegacyPruneCandidate],
    pruned_at: &str,
) -> Vec<LegacyPrunedPart> {
    let mut marked = Vec::new();
    for candidate in candidates {
        let Some(message) = messages.get_mut(candidate.message_index) else {
            continue;
        };
        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some(message_id) = message_id else {
            continue;
        };
        let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let Some(part) = parts.iter_mut().find(|part| {
            part.get("id").and_then(Value::as_str) == Some(candidate.part_id.as_str())
        }) else {
            continue;
        };
        let Some(time) = part.get_mut("time").and_then(Value::as_object_mut) else {
            continue;
        };
        time.insert("pruned".into(), Value::String(pruned_at.to_string()));
        marked.push(LegacyPrunedPart {
            message_index: candidate.message_index,
            message_id,
            part: part.clone(),
        });
    }
    marked
}

fn compact_message_lines(messages: &[Value]) -> String {
    let mut lines = Vec::new();
    for message in messages {
        let kind = message
            .get("type")
            .or_else(|| message.get("role"))
            .and_then(Value::as_str)
            .unwrap_or("message");
        let line = match kind {
            "user" => message
                .get("text")
                .and_then(Value::as_str)
                .map(|text| format!("user: {text}")),
            "assistant" => {
                let text = message
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if text.is_empty() {
                    Some("assistant: [tool/reasoning turn]".into())
                } else {
                    Some(format!("assistant: {text}"))
                }
            }
            "shell" => message
                .get("command")
                .and_then(Value::as_str)
                .map(|command| format!("shell: {command}")),
            "compaction" => message
                .get("summary")
                .and_then(Value::as_str)
                .map(|summary| format!("previous checkpoint: {summary}")),
            _ => Some(format!("{kind}: recorded message")),
        };
        if let Some(mut line) = line {
            // The legacy projection often keeps the useful transcript in
            // structured parts rather than the top-level text field. Retain
            // bounded JSON for files, reasoning, tool calls/results, shell
            // output, and message metadata instead of reducing those turns to
            // "[tool/reasoning turn]".
            let details = match kind {
                "user" => message.get("files").or_else(|| message.get("content")),
                "assistant" => message.get("content"),
                "shell" => message.get("output"),
                "compaction" => message.get("recent"),
                _ => None,
            }
            .filter(|value| !value.is_null())
            .and_then(|value| serde_json::to_string(value).ok());
            if let Some(details) = details {
                line.push_str("\n  details: ");
                line.push_str(&trim_chars(&details, 6_000));
            }
            lines.push(trim_chars(&line, 8_000));
        }
    }
    trim_chars(&lines.join("\n"), 48_000)
}

/// Select the recent tail using the same two controls as the reference
/// compaction policy: preserve at least the configured number of recent user
/// turns, while allowing additional messages that fit the recent-token budget.
/// The server stores JSON messages rather than the typed v1 message graph, so
/// the byte-length estimate is intentionally conservative and deterministic.
fn compaction_recent_count(messages: &[Value], config: &Value) -> usize {
    if messages.is_empty() {
        return 0;
    }
    let compaction = config.get("compaction");
    let tail_turns = compaction
        .and_then(|value| value.get("tail_turns").or_else(|| value.get("tailTurns")))
        .and_then(Value::as_u64)
        .unwrap_or(2)
        .max(1) as usize;
    let preserve_tokens = compaction
        .and_then(|value| {
            value
                .get("preserve_recent_tokens")
                .or_else(|| value.get("preserveRecentTokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(8_000)
        .max(2_000) as usize;

    let mut count = 0usize;
    let mut turns = 0usize;
    let mut estimated_tokens = 0usize;
    for message in messages.iter().rev() {
        let estimated = serde_json::to_string(message)
            .map(|value| value.len().saturating_add(3) / 4)
            .unwrap_or(1)
            .max(1);
        if count > 0
            && turns >= tail_turns
            && estimated_tokens.saturating_add(estimated) > preserve_tokens
        {
            break;
        }
        count += 1;
        estimated_tokens = estimated_tokens.saturating_add(estimated);
        if message
            .get("type")
            .or_else(|| message.get("role"))
            .and_then(Value::as_str)
            == Some("user")
        {
            turns += 1;
        }
    }
    count.min(messages.len()).max(1)
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut result = value.chars().take(max_chars).collect::<String>();
    result.push_str("\n[…truncated…]");
    result
}

impl SessionHistory for ServerHistory {
    fn entries_for_runner(
        &self,
        session_id: &SessionID,
        baseline_seq: u64,
    ) -> Pin<Box<dyn Future<Output = Vec<HistoryEntry>> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let pending_ids = state.pending_session_input_ids(&session_id).await;
            let mut entries = if let Some(entries) =
                durable_history_entries(&state, &session_id, baseline_seq, &pending_ids).await
            {
                entries
            } else {
                let messages = {
                    let stores = state.stores.read().await;
                    let Some(record) = stores.sessions.get(&session_id) else {
                        return Vec::new();
                    };
                    let revert_id = record
                        .info
                        .revert
                        .as_ref()
                        .and_then(|value| value.get("messageID"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let revert_index = revert_id.as_ref().and_then(|id| {
                        record.messages.iter().position(|message| {
                            message.get("id").and_then(Value::as_str) == Some(id.as_str())
                        })
                    });
                    let compact_index = record
                        .messages
                        .iter()
                        .rposition(|message| {
                            message.get("type").and_then(Value::as_str) == Some("compaction")
                        })
                        .filter(|index| revert_index.map(|revert| *index < revert).unwrap_or(true));
                    let start = compact_index.unwrap_or(0);
                    let end = revert_index.unwrap_or(record.messages.len());
                    record.messages[start..end].to_vec()
                };
                messages
                    .into_iter()
                    .filter(|message| {
                        message
                            .get("id")
                            .and_then(Value::as_str)
                            .is_none_or(|id| !pending_ids.contains(id))
                    })
                    .filter_map(|message| server_message(&message))
                    .enumerate()
                    .map(|(index, message)| HistoryEntry {
                        seq: index as u64 + 1,
                        message,
                    })
                    .filter(|entry| entry.seq > baseline_seq)
                    .collect()
            };
            // Plan-mode reminders (reference `session/reminders.ts`) are
            // appended to the latest user message before prompt lowering.
            apply_plan_reminders(&state, &session_id, &mut entries).await;
            entries
        })
    }
}

/// Prefer the event-sourced SQLite history when a production database has
/// already projected it. The v1 in-memory message list remains the fallback
/// while routes still emit only the legacy projection.
async fn durable_history_entries(
    state: &AppState,
    session_id: &SessionID,
    baseline_seq: u64,
    pending_ids: &HashSet<String>,
) -> Option<Vec<HistoryEntry>> {
    let database = state.database.as_ref()?;
    let store = SqliteSessionDb::new(database.as_ref());
    let mut rows = store.message_rows(session_id);
    if rows.is_empty() {
        return None;
    }

    let revert_id = state
        .stores
        .read()
        .await
        .sessions
        .get(session_id)
        .and_then(|record| record.info.revert.as_ref())
        .and_then(|value| value.get("messageID"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(revert_id) = revert_id {
        if let Some(index) = rows.iter().position(|row| row.id == revert_id) {
            rows.truncate(index);
        }
    }
    let compaction = rows
        .iter()
        .rev()
        .find(|row| row.type_ == "compaction")
        .map(|row| row.seq);
    Some(
        oc_session::history::message_rows(&rows, compaction, Some(baseline_seq))
            .into_iter()
            .filter_map(|row| {
                if pending_ids.contains(&row.id) {
                    return None;
                }
                let mut value = row.data;
                if let Some(object) = value.as_object_mut() {
                    object.insert("id".into(), Value::String(row.id));
                    object.insert("type".into(), Value::String(row.type_));
                }
                server_message(&value).map(|message| HistoryEntry {
                    seq: row.seq,
                    message,
                })
            })
            .collect(),
    )
}

/// Append plan-mode / build-mode reminders to the latest user message,
/// mirroring `reference/packages/opencode/src/session/reminders.ts:apply`.
/// The plan file path and lifecycle follow `Session.plan()` (session.ts): a
/// VCS worktree keeps plans under `.opencode/plans`, otherwise they live in
/// the global data directory. The plan file's parent directory is created when
/// the plan agent is active and no plan exists yet, so the `write` tool can
/// create it.
async fn apply_plan_reminders(state: &AppState, session_id: &str, entries: &mut [HistoryEntry]) {
    let record = {
        let stores = state.stores.read().await;
        stores.sessions.get(session_id).cloned()
    };
    let Some(record) = record else {
        return;
    };
    let agent = record
        .info
        .agent
        .clone()
        .unwrap_or_else(|| "build".to_string());

    // Locate the last assistant agent and the latest user message.
    let mut last_assistant_agent: Option<String> = None;
    for entry in entries.iter().rev() {
        if let SessionMessage::Assistant(assistant) = &entry.message {
            last_assistant_agent = Some(assistant.agent.clone());
            break;
        }
    }
    let was_plan = entries.iter().any(|entry| {
        matches!(&entry.message, SessionMessage::Assistant(assistant) if assistant.agent == "plan")
    });
    let Some(user_index) = entries
        .iter()
        .rposition(|entry| matches!(entry.message, SessionMessage::User(_)))
    else {
        return;
    };

    let directory = &record.info.location.directory;
    let worktree = git_worktree(directory);
    let has_vcs = worktree.is_some();
    let plan_base = worktree.as_deref().unwrap_or(directory);
    let created = record.info.time.created;
    let slug = oc_session::plan::slug_from_session_id(session_id);
    let data_dir = oc_mcp::auth::default_data_dir()
        .to_string_lossy()
        .into_owned();

    // `ensureDir` runs only while the plan agent is active on a fresh plan
    // (reference reminders.ts experimental path).
    let ensure_dir = agent == "plan" && last_assistant_agent.as_deref() != Some("plan");
    let plan = oc_session::plan::ensure_plan_file(
        plan_base,
        &data_dir,
        has_vcs,
        created.max(0) as u64,
        &slug,
        ensure_dir,
    );
    let ctx = oc_session::reminders::ReminderContext::from_plan_file(session_id, &plan);
    let experimental = plan_mode_experimental(&state.stores.read().await.config);
    let Some(text) = oc_session::reminders::reminder_text(
        &agent,
        last_assistant_agent.as_deref(),
        was_plan,
        &ctx,
        experimental,
    ) else {
        return;
    };

    if let SessionMessage::User(user) = &mut entries[user_index].message {
        if !user.text.trim().is_empty() {
            user.text.push('\n');
        }
        user.text.push_str(&text);
    }
}

/// Resolve the git worktree root for a directory, if it is inside a VCS repo.
fn git_worktree(directory: &str) -> Option<String> {
    let mut current = std::path::PathBuf::from(directory);
    loop {
        if current.join(".git").exists() {
            let output = std::process::Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(&current)
                .output()
                .ok()?;
            if output.status.success() {
                let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !root.is_empty() {
                    return Some(root);
                }
            }
            return Some(current.to_string_lossy().into_owned());
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return None,
        }
    }
}

/// `RuntimeFlags.experimentalPlanMode`: config `experimental.plan_mode` (or
/// `experimental_plan_mode`), falling back to `OPENCODE_EXPERIMENTAL_PLAN_MODE`.
fn plan_mode_experimental(config: &Value) -> bool {
    let experimental = config.get("experimental").and_then(Value::as_object);
    experimental
        .and_then(|experimental| {
            experimental
                .get("plan_mode")
                .or_else(|| experimental.get("planMode"))
        })
        .and_then(Value::as_bool)
        .or_else(|| {
            config
                .get("experimental_plan_mode")
                .and_then(Value::as_bool)
        })
        .or_else(|| {
            std::env::var("OPENCODE_EXPERIMENTAL_PLAN_MODE")
                .ok()
                .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        })
        .unwrap_or(false)
}

#[derive(Clone)]
struct ServerLocation {
    state: AppState,
    session_id: String,
}

impl LocationService for ServerLocation {
    fn current(&self) -> RunnerLocation {
        let session_location = self.state.stores.try_read().ok().and_then(|stores| {
            stores
                .sessions
                .get(&self.session_id)
                .map(|record| record.info.location.clone())
        });
        RunnerLocation::new(
            session_location
                .as_ref()
                .map(|location| location.directory.clone())
                .unwrap_or_else(|| self.state.location.directory.clone()),
            session_location
                .and_then(|location| location.workspace_id)
                .or_else(|| self.state.location.workspace_id.clone()),
        )
    }
}

#[derive(Clone)]
struct ServerAgents {
    state: AppState,
}

impl Agents for ServerAgents {
    fn select(
        &self,
        id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = AgentSelection> + Send + '_>> {
        let id = id.unwrap_or("build").to_string();
        let state = self.state.clone();
        Box::pin(async move {
            let config = state.stores.read().await.config.clone();
            let configured = config
                .get("agent")
                .and_then(Value::as_object)
                .and_then(|agents| agents.get(&id));
            let system = configured
                .and_then(|agent| agent.get("system").or_else(|| agent.get("prompt")))
                .and_then(Value::as_str)
                .map(str::to_string);
            let steps = configured
                .and_then(|agent| agent.get("steps"))
                .and_then(Value::as_u64)
                .and_then(|steps| u32::try_from(steps).ok());
            let permissions = configured
                .and_then(|agent| agent.get("permission"))
                .and_then(Value::as_object)
                .map(|permissions| permissions.keys().cloned().collect())
                .unwrap_or_default();
            AgentSelection {
                id,
                info: Some(AgentInfo {
                    system,
                    steps,
                    permissions,
                }),
            }
        })
    }
}

#[derive(Clone)]
struct ServerModel {
    state: AppState,
}

impl SessionRunnerModel for ServerModel {
    fn resolve(
        &self,
        session: &RunnerSessionInfo,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<oc_session_runner::llm::message::Model, ModelError>>
                + Send
                + '_,
        >,
    > {
        let session = session.clone();
        let state = self.state.clone();
        Box::pin(async move {
            let config = state.stores.read().await.config.clone();
            let configured_agent_model = session.agent.as_deref().and_then(|agent| {
                config
                    .get("agent")
                    .and_then(Value::as_object)
                    .and_then(|agents| agents.get(agent))
                    .and_then(|agent| agent.get("model"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            let configured_root_model = config
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string);
            let configured_model = session
                .model
                .as_ref()
                .map(|model| format!("{}/{}", model.provider_id, model.id))
                .filter(|model| !model.starts_with('/') && !model.ends_with('/'))
                .or(configured_agent_model)
                .or(configured_root_model)
                .or_else(|| std::env::var("OPENCODE_MODEL").ok())
                .unwrap_or_else(|| "openai/gpt-4o-mini".into());
            let (provider, model) = configured_model
                .split_once('/')
                .map(|(provider, model)| (provider.to_string(), model.to_string()))
                .filter(|(provider, model)| !provider.is_empty() && !model.is_empty())
                .ok_or_else(|| {
                    ModelError::NotSelected(ModelNotSelectedError {
                        session_id: session.id.clone(),
                    })
                })?;
            let resolved = oc_session_runner::llm::message::Model::make(&model, &provider);
            let (catalog_cost, catalog_limits) = catalog_model_metadata(&provider, &model);
            let (configured_cost, configured_limits) =
                configured_model_metadata(&config, &provider, &model);
            let cost = configured_cost.or(catalog_cost);
            let limits = configured_limits.or(catalog_limits);
            let resolved = match cost {
                Some(cost) => resolved.with_cost(cost),
                None => resolved,
            };
            Ok(match limits {
                Some(limits) => resolved.with_limits(limits),
                None => resolved,
            })
        })
    }
}

fn catalog_model_metadata(
    provider: &str,
    model: &str,
) -> (Option<RunnerModelCost>, Option<RunnerModelLimits>) {
    let Ok(catalog) = oc_provider::models_dev::snapshot() else {
        return (None, None);
    };
    let Some(model_info) = catalog
        .get(provider)
        .and_then(|provider| provider.models.get(model))
    else {
        return (None, None);
    };
    let cost = model_info.cost.as_ref().map(|cost| RunnerModelCost {
        input: cost.input.unwrap_or(0.0),
        output: cost.output.unwrap_or(0.0),
        cache_read: cost.cache_read.unwrap_or(0.0),
        cache_write: cost.cache_write.unwrap_or(0.0),
    });
    let limits = model_info.limit.as_ref().and_then(|limit| {
        let context = limit.context.and_then(token_limit);
        let input = limit.input.and_then(token_limit);
        let output = limit.output.and_then(token_limit);
        (context.is_some() || input.is_some() || output.is_some()).then_some(RunnerModelLimits {
            context,
            input,
            output,
        })
    });
    (cost, limits)
}

fn configured_model_metadata(
    config: &Value,
    provider: &str,
    model: &str,
) -> (Option<RunnerModelCost>, Option<RunnerModelLimits>) {
    let Some(model_info) = config
        .get("provider")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(provider))
        .and_then(Value::as_object)
        .and_then(|provider| provider.get("models"))
        .and_then(Value::as_object)
        .and_then(|models| models.get(model))
        .and_then(Value::as_object)
    else {
        return (None, None);
    };

    let number = |object: Option<&serde_json::Map<String, Value>>, key: &str| {
        object
            .and_then(|object| object.get(key))
            .and_then(Value::as_f64)
    };
    let cost_object = model_info.get("cost").and_then(Value::as_object);
    let cost = cost_object.and_then(|cost| {
        let input = number(Some(cost), "input")?;
        Some(RunnerModelCost {
            input,
            output: number(Some(cost), "output").unwrap_or(0.0),
            cache_read: number(Some(cost), "cacheRead")
                .or_else(|| number(Some(cost), "cache_read"))
                .unwrap_or(0.0),
            cache_write: number(Some(cost), "cacheWrite")
                .or_else(|| number(Some(cost), "cache_write"))
                .unwrap_or(0.0),
        })
    });
    let limit_object = model_info
        .get("limit")
        .or_else(|| model_info.get("limits"))
        .and_then(Value::as_object);
    let limits = limit_object.and_then(|limit| {
        let context = number(Some(limit), "context").and_then(token_limit);
        let input = number(Some(limit), "input").and_then(token_limit);
        let output = number(Some(limit), "output").and_then(token_limit);
        (context.is_some() || input.is_some() || output.is_some()).then_some(RunnerModelLimits {
            context,
            input,
            output,
        })
    });
    (cost, limits)
}

fn token_limit(value: f64) -> Option<u64> {
    value.is_finite().then_some(value.max(0.0).round() as u64)
}

const FALLBACK_COMPACTION_ENTRIES: usize = 40;
const FALLBACK_COMPACTION_BYTES: usize = 120_000;
const COMPACTION_TOKEN_BYTES: usize = 4;
const COMPACTION_BUFFER_TOKENS: u64 = 20_000;

/// Estimate the prompt size and apply the reference model-aware overflow
/// policy. The runner does not own a tokenizer, so serialized history is
/// conservatively converted at four bytes per token. Unknown models retain the
/// legacy size/entry fallback rather than disabling automatic compaction.
fn compaction_needed(input: &CompactionInput, estimated_size: usize) -> bool {
    let Some(limits) = input.model.limits.as_ref() else {
        return input.entries.len() >= FALLBACK_COMPACTION_ENTRIES
            || estimated_size >= FALLBACK_COMPACTION_BYTES;
    };

    let Some(context) = limits.context else {
        return input.entries.len() >= FALLBACK_COMPACTION_ENTRIES
            || estimated_size >= FALLBACK_COMPACTION_BYTES;
    };
    let output = input
        .request
        .generation
        .as_ref()
        .and_then(|generation| generation.max_tokens)
        .or(limits.output)
        .unwrap_or(COMPACTION_BUFFER_TOKENS);
    let reserved = COMPACTION_BUFFER_TOKENS.min(output);
    let usable = limits.input.unwrap_or(context).saturating_sub(reserved);
    let estimated_tokens = estimated_size
        .saturating_add(COMPACTION_TOKEN_BYTES - 1)
        .checked_div(COMPACTION_TOKEN_BYTES)
        .unwrap_or(u64::MAX as usize) as u64;
    estimated_tokens >= usable
}

struct ServerLlm {
    state: AppState,
}

impl LlmClient for ServerLlm {
    fn stream(
        &self,
        request: LLMRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LlmEventStream, LLMError>> + Send + '_>> {
        Box::pin(async move {
            if request.model.provider == "stub" {
                let prompt = request
                    .messages
                    .iter()
                    .rev()
                    .flat_map(|message| message.content.iter())
                    .find_map(|part| match part {
                        RunnerContentPart::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "hello".into());
                let events = vec![
                    oc_session_runner::llm::LLMEvent::StepStart { index: 0.0 },
                    oc_session_runner::llm::LLMEvent::TextStart {
                        id: "stub_text".into(),
                        provider_metadata: None,
                    },
                    oc_session_runner::llm::LLMEvent::TextDelta {
                        id: "stub_text".into(),
                        text: format!("stub: {prompt}"),
                        provider_metadata: None,
                    },
                    oc_session_runner::llm::LLMEvent::TextEnd {
                        id: "stub_text".into(),
                        provider_metadata: None,
                    },
                    oc_session_runner::llm::LLMEvent::StepFinish {
                        index: 0.0,
                        reason: "stop".into(),
                        usage: Some(Default::default()),
                        provider_metadata: None,
                    },
                ];
                return Ok(
                    Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as LlmEventStream
                );
            }
            let config = self.state.stores.read().await.config.clone();
            crate::handlers::provider::refresh_provider_auth(&self.state, &request.model.provider)
                .map_err(runner_transport_error)?;
            let auth = crate::handlers::provider::provider_auth(&request.model.provider);
            let native_base_url = if request.model.provider == "openai"
                && crate::handlers::provider::provider_uses_oauth(&request.model.provider)
            {
                Some("https://chatgpt.com/backend-api/codex".to_string())
            } else if request.model.provider == "github-copilot" {
                crate::handlers::provider::provider_oauth_base_url(&request.model.provider)
            } else {
                None
            };
            let model = crate::instance_handlers::configured_model_for_config_with_auth_and_base(
                &config,
                &request.model.provider,
                &request.model.id,
                auth,
                native_base_url.as_deref(),
            )
            .map_err(runner_transport_error)?;
            let messages = request
                .messages
                .into_iter()
                .map(json_convert::<oc_llm::Message>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(runner_transport_error)?;
            let tools = request
                .tools
                .into_iter()
                .map(json_convert::<oc_llm::ToolDefinition>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(runner_transport_error)?;
            let system = request
                .system
                .into_iter()
                .map(|part| oc_llm::SystemPart::make(part.text))
                .collect::<Vec<_>>();

            let mut input = oc_llm::RequestInput::new(model);
            input.id = request.id;
            input.system = Some(oc_llm::SystemPartRef::Many(system));
            input.messages = Some(messages);
            input.tools = Some(tools);
            input.tool_choice = request.tool_choice.as_ref().map(tool_choice);
            input.generation = request.generation.map(generation_options);
            input.provider_options = request
                .provider_options
                .as_ref()
                .and_then(|options| json_convert::<oc_llm::ProviderOptions>(options).ok());
            input.http = request.http.map(http_options);

            let client = oc_llm::LlmClient::new();
            let stream = client.stream(oc_llm::request(input));
            let mapped = stream.map(|event| {
                let event = event.map_err(runner_llm_error)?;
                json_convert::<oc_session_runner::llm::LLMEvent>(&event)
                    .map_err(runner_transport_error)
            });
            Ok(Box::pin(mapped) as LlmEventStream)
        })
    }
}

fn json_convert<T: DeserializeOwned>(value: impl Serialize) -> Result<T, String> {
    serde_json::from_value(serde_json::to_value(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

fn runner_transport_error(message: String) -> LLMError {
    LLMError {
        module: "oc-server".into(),
        method: "stream".into(),
        reason: LLMErrorReason::Transport(ReasonMessage {
            message,
            provider_metadata: None,
            http: None,
        }),
    }
}

fn runner_llm_error(error: oc_llm::LlmError) -> LLMError {
    let module = error.module;
    let method = error.method;
    let reason = match error.reason {
        oc_llm::LlmErrorReason::InvalidRequest {
            message,
            parameter,
            classification,
            provider_metadata,
            http,
        } => LLMErrorReason::InvalidRequest(oc_session_runner::llm::error::InvalidRequestReason {
            message,
            parameter,
            classification: classification.map(runner_failure_classification),
            provider_metadata: provider_metadata.map(runner_provider_metadata),
            http: http.and_then(runner_http_context),
        }),
        oc_llm::LlmErrorReason::NoRoute {
            route,
            provider,
            model,
        } => LLMErrorReason::NoRoute(ReasonMessage {
            message: format!("No LLM route for {provider}/{model} using {route}"),
            provider_metadata: None,
            http: None,
        }),
        oc_llm::LlmErrorReason::Authentication {
            message,
            kind,
            provider_metadata,
            http,
        } => LLMErrorReason::Authentication(oc_session_runner::llm::error::AuthenticationReason {
            message,
            kind: runner_authentication_kind(kind),
            provider_metadata: provider_metadata.map(runner_provider_metadata),
            http: http.and_then(runner_http_context),
        }),
        oc_llm::LlmErrorReason::RateLimit {
            message,
            retry_after_ms,
            rate_limit: _,
            provider_metadata,
            http,
        } => LLMErrorReason::RateLimit(oc_session_runner::llm::error::RateLimitReason {
            message,
            retry_after_ms: retry_after_ms.map(|value| value as f64),
            provider_metadata: provider_metadata.map(runner_provider_metadata),
            http: http.and_then(runner_http_context),
        }),
        oc_llm::LlmErrorReason::QuotaExceeded {
            message,
            provider_metadata,
            http,
        } => LLMErrorReason::QuotaExceeded(ReasonMessage {
            message,
            provider_metadata: provider_metadata.map(runner_provider_metadata),
            http: http.and_then(runner_http_context),
        }),
        oc_llm::LlmErrorReason::ContentPolicy {
            message,
            provider_metadata,
            http,
        } => LLMErrorReason::ContentPolicy(ReasonMessage {
            message,
            provider_metadata: provider_metadata.map(runner_provider_metadata),
            http: http.and_then(runner_http_context),
        }),
        oc_llm::LlmErrorReason::ProviderInternal {
            message,
            status,
            retry_after_ms,
            provider_metadata,
            http,
        } => LLMErrorReason::ProviderInternal(
            oc_session_runner::llm::error::ProviderInternalReason {
                message,
                status: status as f64,
                retry_after_ms: retry_after_ms.map(|value| value as f64),
                provider_metadata: provider_metadata.map(runner_provider_metadata),
                http: http.and_then(runner_http_context),
            },
        ),
        oc_llm::LlmErrorReason::Transport { message, http, .. } => {
            LLMErrorReason::Transport(ReasonMessage {
                message,
                provider_metadata: None,
                http: http.and_then(runner_http_context),
            })
        }
        oc_llm::LlmErrorReason::InvalidProviderOutput {
            message,
            provider_metadata,
            ..
        } => LLMErrorReason::InvalidProviderOutput(ReasonMessage {
            message,
            provider_metadata: provider_metadata.map(runner_provider_metadata),
            http: None,
        }),
        oc_llm::LlmErrorReason::UnknownProvider {
            message,
            status,
            provider_metadata,
            http,
        } => LLMErrorReason::UnknownProvider(ReasonMessage {
            message: status
                .map(|status| format!("HTTP {status}: {message}"))
                .unwrap_or(message),
            provider_metadata: provider_metadata.map(runner_provider_metadata),
            http: http.and_then(runner_http_context),
        }),
    };
    LLMError {
        module,
        method,
        reason,
    }
}

fn runner_failure_classification(
    classification: oc_llm::ProviderFailureClassification,
) -> oc_session_runner::llm::ProviderFailureClassification {
    match classification {
        oc_llm::ProviderFailureClassification::ContextOverflow => {
            oc_session_runner::llm::ProviderFailureClassification::ContextOverflow
        }
    }
}

fn runner_authentication_kind(
    kind: oc_llm::AuthKind,
) -> oc_session_runner::llm::error::AuthenticationKind {
    match kind {
        oc_llm::AuthKind::Missing => oc_session_runner::llm::error::AuthenticationKind::Missing,
        oc_llm::AuthKind::Invalid => oc_session_runner::llm::error::AuthenticationKind::Invalid,
        oc_llm::AuthKind::Expired => oc_session_runner::llm::error::AuthenticationKind::Expired,
        oc_llm::AuthKind::InsufficientPermissions => {
            oc_session_runner::llm::error::AuthenticationKind::InsufficientPermissions
        }
        oc_llm::AuthKind::Unknown => oc_session_runner::llm::error::AuthenticationKind::Unknown,
    }
}

fn runner_provider_metadata(
    metadata: oc_llm::ProviderMetadata,
) -> oc_session_runner::llm::ProviderMetadata {
    metadata
        .into_iter()
        .map(|(provider, fields)| (provider, Value::Object(fields)))
        .collect()
}

fn runner_http_context(
    context: oc_llm::HttpContext,
) -> Option<oc_session_runner::llm::error::HttpContext> {
    let request = context.request?;
    Some(oc_session_runner::llm::error::HttpContext {
        request: oc_session_runner::llm::error::HttpRequestDetails {
            method: request.method,
            url: request.url,
            headers: runner_json_headers(request.headers),
        },
        response: context.response.map(|response| {
            oc_session_runner::llm::error::HttpResponseDetails {
                status: response.status as f64,
                headers: runner_json_headers(response.headers),
            }
        }),
        body: context.body,
        body_truncated: context.body_truncated,
        request_id: context.request_id,
        rate_limit: context.rate_limit.map(runner_rate_limit),
    })
}

fn runner_rate_limit(
    details: oc_llm::HttpRateLimitDetails,
) -> oc_session_runner::llm::error::HttpRateLimitDetails {
    oc_session_runner::llm::error::HttpRateLimitDetails {
        retry_after_ms: details.retry_after_ms.map(|value| value as f64),
        limit: details.limit.map(runner_json_headers),
        remaining: details.remaining.map(runner_json_headers),
        reset: details.reset.map(runner_json_headers),
    }
}

fn runner_json_headers(
    headers: std::collections::BTreeMap<String, String>,
) -> serde_json::Map<String, Value> {
    headers
        .into_iter()
        .map(|(name, value)| (name, Value::String(value)))
        .collect()
}

fn tool_choice(choice: &RunnerToolChoice) -> oc_llm::ToolChoiceInput {
    let value = match choice.kind {
        oc_session_runner::llm::message::ToolChoiceKind::Auto => "auto",
        oc_session_runner::llm::message::ToolChoiceKind::None => "none",
        oc_session_runner::llm::message::ToolChoiceKind::Required => "required",
        oc_session_runner::llm::message::ToolChoiceKind::Tool => {
            return oc_llm::ToolChoiceInput::String(
                choice.name.clone().unwrap_or_else(|| "auto".into()),
            )
        }
    };
    oc_llm::ToolChoiceInput::String(value.into())
}

fn generation_options(options: RunnerGenerationOptions) -> oc_llm::GenerationOptions {
    oc_llm::GenerationOptions {
        temperature: options.temperature,
        top_p: options.top_p,
        top_k: options.top_k.map(|value| value as i64),
        max_tokens: options.max_tokens.map(|value| value as i64),
        ..Default::default()
    }
}

fn http_options(options: RunnerHttpOptions) -> oc_llm::HttpOptions {
    oc_llm::HttpOptions {
        body: None,
        headers: options.headers.map(|headers| {
            headers
                .into_iter()
                .map(|(key, value)| (key, value.as_str().unwrap_or_default().to_string()))
                .collect()
        }),
        query: None,
    }
}

struct ServerInput {
    state: AppState,
}

impl SessionInput for ServerInput {
    fn has_pending(
        &self,
        session_id: &SessionID,
        delivery: Delivery,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            state
                .pending_session_input(&session_id, delivery.as_str())
                .await
        })
    }

    fn promote_steers(
        &self,
        session_id: &SessionID,
        cutoff: u64,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move { state.promote_session_steers(&session_id, cutoff).await })
    }

    fn promote_next_queued(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move { state.promote_next_session_queue(&session_id).await })
    }

    fn latest_sequence(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move { state.latest_session_sequence(&session_id).await })
    }
}

struct EmptySystemContext;

impl SystemContextRegistry for EmptySystemContext {
    fn load(&self) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>> {
        Box::pin(async { SystemContext::default() })
    }
}

struct ServerSkillGuidance {
    state: AppState,
    session_id: String,
}

impl SkillGuidance for ServerSkillGuidance {
    fn load(&self, agent: &str) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>> {
        let state = self.state.clone();
        let session_id = self.session_id.clone();
        let agent = agent.to_string();
        Box::pin(async move {
            let (directory, config) = {
                let stores = state.stores.read().await;
                let directory = stores
                    .sessions
                    .get(&session_id)
                    .map(|record| record.info.location.directory.clone())
                    .unwrap_or_else(|| state.location.directory.clone());
                (directory, stores.config.clone())
            };
            load_skill_guidance(&directory, &config, &agent).await
        })
    }
}

async fn load_skill_guidance(directory: &str, config: &Value, agent: &str) -> SystemContext {
    let directory_path = PathBuf::from(directory);
    let worktree = skill_worktree(&directory_path).await;
    let home = oc_command::global::Global::detect().home;
    let (paths, urls) = configured_skill_sources(config);
    let mut pulled_dirs = Vec::new();

    if !urls.is_empty() {
        let discovery = oc_command::skill::discovery::Discovery::new(
            oc_command::global::Global::with_home(home.clone())
                .cache
                .join("skills"),
        );
        for url in urls {
            match discovery.pull(&url).await {
                Ok(dirs) => pulled_dirs.extend(dirs),
                Err(error) => {
                    tracing::warn!(url = %url, error = %error, "skill discovery failed; omitting skill guidance");
                    return SystemContext::default();
                }
            }
        }
    }

    let settings = oc_command::skill::Settings {
        home,
        directory: directory_path,
        worktree,
        disable_external_skills: false,
        disable_claude_code_skills: false,
        paths,
        pulled_dirs,
        config_dirs: None,
    };
    let service = match oc_command::skill::SkillService::load_with_environment(&settings) {
        Ok(service) => service,
        Err(error) => {
            tracing::warn!(error = %error, "skill discovery failed; omitting skill guidance");
            return SystemContext::default();
        }
    };

    let available = service
        .available(None)
        .into_iter()
        .map(|skill| oc_session::system::SkillInfo {
            name: skill.name.clone(),
            description: skill.description.clone(),
            location: skill.location.clone(),
        })
        .collect::<Vec<_>>();
    let rules = configured_permission_rules(config, agent);
    oc_session::system::skills(&rules, &available)
        .map(|baseline| SystemContext { baseline })
        .unwrap_or_default()
}

async fn skill_worktree(directory: &Path) -> PathBuf {
    let directory = directory.to_path_buf();
    let fallback = directory.clone();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(&directory)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                (!root.is_empty()).then(|| PathBuf::from(root))
            })
            .unwrap_or(directory)
    })
    .await
    .unwrap_or_else(|error| {
        tracing::debug!(error = %error, "failed to resolve skill worktree; using session directory");
        fallback
    })
}

fn configured_skill_sources(config: &Value) -> (Vec<String>, Vec<String>) {
    let skills = config.get("skills").and_then(Value::as_object);
    let values = |key: &str| {
        skills
            .and_then(|skills| skills.get(key))
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    (values("paths"), values("urls"))
}

fn configured_permission_rules(config: &Value, agent: &str) -> oc_session::v1::Ruleset {
    let mut rules = Vec::new();
    append_tool_permission_rules(config.get("tools"), &mut rules);
    append_permission_rules(config.get("permission"), &mut rules);
    if let Some(agent_config) = config
        .get("agent")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(agent))
    {
        append_tool_permission_rules(agent_config.get("tools"), &mut rules);
        append_permission_rules(agent_config.get("permission"), &mut rules);
    }
    rules
}

fn append_permission_rules(value: Option<&Value>, rules: &mut oc_session::v1::Ruleset) {
    let Some(value) = value else { return };
    match value {
        Value::String(action) => rules.push(oc_session::v1::PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: action.clone(),
        }),
        Value::Object(entries) => {
            for (permission, action) in entries {
                match action {
                    Value::String(action) => rules.push(oc_session::v1::PermissionRule {
                        permission: permission.clone(),
                        pattern: "*".into(),
                        action: action.clone(),
                    }),
                    Value::Object(patterns) => {
                        for (pattern, action) in patterns {
                            if let Some(action) = action.as_str() {
                                rules.push(oc_session::v1::PermissionRule {
                                    permission: permission.clone(),
                                    pattern: pattern.clone(),
                                    action: action.to_string(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn append_tool_permission_rules(value: Option<&Value>, rules: &mut oc_session::v1::Ruleset) {
    let Some(Value::Object(tools)) = value else {
        return;
    };
    for (tool, enabled) in tools {
        let Some(enabled) = enabled.as_bool() else {
            continue;
        };
        let permission = match tool.as_str() {
            "write" | "edit" | "patch" => "edit",
            other => other,
        };
        rules.push(oc_session::v1::PermissionRule {
            permission: permission.to_string(),
            pattern: "*".into(),
            action: if enabled { "allow" } else { "deny" }.into(),
        });
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
struct ToolPermissionRequest {
    permission: String,
    pattern: String,
    fallback_ask: bool,
    metadata: Value,
}

#[allow(dead_code)]
fn tool_permission_request(name: &str, input: &Value) -> Option<ToolPermissionRequest> {
    let path = input
        .get("path")
        .or_else(|| input.get("filePath"))
        .or_else(|| input.get("workdir"))
        .and_then(Value::as_str);
    let path_pattern = || path.unwrap_or("*").to_string();
    let input_pattern = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("*")
            .to_string()
    };

    let (permission, pattern, fallback_ask, metadata) = match name {
        "read" => (
            "read".into(),
            path_pattern(),
            false,
            json!({ "tool": name }),
        ),
        "glob" => (
            "glob".into(),
            input_pattern("pattern"),
            false,
            json!({ "tool": name }),
        ),
        "grep" => (
            "grep".into(),
            input_pattern("pattern"),
            false,
            json!({ "tool": name }),
        ),
        "write" | "edit" | "apply_patch" => {
            ("edit".into(), path_pattern(), true, json!({ "tool": name }))
        }
        "bash" => (
            "bash".into(),
            input_pattern("command"),
            true,
            json!({ "tool": "bash" }),
        ),
        "task" => (
            "task".into(),
            input_pattern("subagent_type"),
            true,
            json!({
                "tool": "task",
                "description": input.get("description").cloned().unwrap_or(Value::Null),
            }),
        ),
        "question" => (
            "question".into(),
            "*".into(),
            false,
            json!({ "tool": name }),
        ),
        oc_tool::core::plan::NAME => (
            "plan_exit".into(),
            "*".into(),
            false,
            json!({ "tool": name }),
        ),
        "lsp" => ("lsp".into(), path_pattern(), false, json!({ "tool": name })),
        "todowrite" | "webfetch" | "websearch" | "skill" => (
            name.to_string(),
            input_pattern("path"),
            false,
            json!({ "tool": name }),
        ),
        oc_session::tools::MCP_RESOURCE_TOOLS_LIST
        | oc_session::tools::MCP_RESOURCE_TOOLS_LIST_TEMPLATES
        | oc_session::tools::MCP_RESOURCE_TOOLS_READ => (
            "read".into(),
            input_pattern("uri"),
            false,
            json!({ "tool": name }),
        ),
        _ => return None,
    };

    Some(ToolPermissionRequest {
        permission,
        pattern,
        fallback_ask,
        metadata,
    })
}

#[allow(dead_code)]
fn tool_external_paths(name: &str, input: &Value) -> Vec<(String, bool)> {
    let path = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match name {
        "bash" => path("workdir")
            .map(|path| vec![(path, true)])
            .unwrap_or_default(),
        "glob" | "grep" => path("path")
            .map(|path| vec![(path, true)])
            .unwrap_or_default(),
        "read" | "write" | "edit" => path("path")
            .map(|path| vec![(path, false)])
            .unwrap_or_default(),
        "lsp" => path("filePath")
            .map(|path| vec![(path, false)])
            .unwrap_or_default(),
        "apply_patch" => input
            .get("patchText")
            .and_then(Value::as_str)
            .and_then(|patch| oc_tool::patch::parse_patch(patch).ok())
            .map(|hunks| {
                hunks
                    .into_iter()
                    .flat_map(|hunk| {
                        let mut paths = vec![(hunk.path().to_string(), false)];
                        if let oc_tool::patch::Hunk::Update {
                            move_path: Some(move_path),
                            ..
                        } = hunk
                        {
                            paths.push((move_path, false));
                        }
                        paths
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[allow(dead_code)]
fn external_directory_pattern(location: &str, path: &str, directory: bool) -> String {
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        Path::new(location).join(path)
    };
    let directory = if directory
        || std::fs::symlink_metadata(&resolved)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    {
        resolved
    } else {
        resolved.parent().map(Path::to_path_buf).unwrap_or(resolved)
    };
    format!("{}/*", directory.to_string_lossy()).replace('\\', "/")
}

struct EmptyReferenceGuidance;

impl ReferenceGuidance for EmptyReferenceGuidance {
    fn load(&self) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>> {
        Box::pin(async { SystemContext::default() })
    }
}

struct EmptySnapshots;

impl Snapshots for EmptySnapshots {
    fn capture(&self) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        Box::pin(async { None })
    }

    fn files(
        &self,
        _from: &str,
        _to: &str,
    ) -> Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + '_>> {
        Box::pin(async { None })
    }
}

struct ServerContextEpoch {
    state: AppState,
}

impl SessionContextEpoch for ServerContextEpoch {
    fn initialize(
        &self,
        session_id: &SessionID,
        context: SystemContext,
    ) -> Pin<Box<dyn Future<Output = Option<PreparedContext>> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move { Some(load_or_create_context_epoch(&state, &session_id, context)) })
    }

    fn prepare(
        &self,
        session_id: &SessionID,
        context: SystemContext,
    ) -> Pin<Box<dyn Future<Output = PreparedContext> + Send + '_>> {
        let state = self.state.clone();
        let session_id = session_id.clone();
        Box::pin(async move { load_or_create_context_epoch(&state, &session_id, context) })
    }
}

fn load_or_create_context_epoch(
    state: &AppState,
    session_id: &SessionID,
    context: SystemContext,
) -> PreparedContext {
    let Some(database) = state.database.as_ref() else {
        return PreparedContext {
            baseline: context.baseline,
            baseline_seq: 0,
        };
    };
    let store = SqliteSessionDb::new(database.as_ref());
    if let Ok(Some(epoch)) = database.context_epoch(session_id) {
        return PreparedContext {
            baseline: epoch.baseline,
            baseline_seq: epoch.baseline_seq.max(0) as u64,
        };
    }

    let baseline_seq = store
        .message_rows(session_id)
        .last()
        .map(|row| row.seq)
        .unwrap_or(0);
    let row = oc_database::tables::SessionContextEpochRow {
        session_id: session_id.clone(),
        baseline: context.baseline.clone(),
        snapshot: json!({}),
        baseline_seq: baseline_seq.min(i64::MAX as u64) as i64,
    };
    if let Err(error) = database.upsert(
        "session_context_epoch",
        &row,
        oc_database::tables::json_columns("session_context_epoch"),
        "session_id",
        &oc_database::Value::Text(session_id.clone()),
    ) {
        tracing::warn!(
            session_id,
            ?error,
            "failed to persist session context epoch"
        );
    }
    PreparedContext {
        baseline: context.baseline,
        baseline_seq,
    }
}

struct ServerCompaction {
    state: AppState,
}

fn auto_compaction_enabled(config: &Value) -> bool {
    config
        .get("compaction")
        .and_then(|compaction| compaction.get("auto"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn compaction_model_ref(model: &RunnerLlmModel) -> Option<ModelRef> {
    let id = model.id.trim();
    let provider_id = model.provider.trim();
    if id.is_empty() || provider_id.is_empty() {
        return None;
    }
    Some(ModelRef {
        id: id.to_string(),
        provider_id: provider_id.to_string(),
        variant: None,
    })
}

async fn compact_session_with_provider_fallback(
    state: &AppState,
    session_id: &str,
    reason: CompactionReason,
    model: &RunnerLlmModel,
) -> bool {
    let Some(model) = compaction_model_ref(model) else {
        return compact_session(state, session_id, reason).await;
    };

    match summarize_and_compact_session(state, session_id, reason, Some(model)).await {
        Ok(compacted) => compacted,
        Err(error) => {
            tracing::warn!(
                session_id,
                %error,
                "provider-backed automatic compaction failed; using deterministic fallback"
            );
            compact_session(state, session_id, reason).await
        }
    }
}

impl SessionCompaction for ServerCompaction {
    fn compact_if_needed(
        &self,
        input: CompactionInput,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let state = self.state.clone();
        Box::pin(async move {
            let auto_enabled = {
                let stores = state.stores.read().await;
                auto_compaction_enabled(&stores.config)
            };
            if !auto_enabled {
                return false;
            }
            let estimated_size = input
                .entries
                .iter()
                .filter_map(|entry| serde_json::to_string(&entry.message).ok())
                .map(|message| message.len())
                .sum::<usize>();
            if !compaction_needed(&input, estimated_size) {
                return false;
            }
            compact_session_with_provider_fallback(
                &state,
                &input.session_id,
                CompactionReason::Auto,
                &input.model,
            )
            .await
        })
    }

    fn compact_after_overflow(
        &self,
        input: CompactionInput,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let state = self.state.clone();
        Box::pin(async move {
            compact_session_with_provider_fallback(
                &state,
                &input.session_id,
                CompactionReason::Auto,
                &input.model,
            )
            .await
        })
    }
}

/// Built-ins are exposed through one permission-aware settlement hook. Read
/// operations inside the active workspace are admitted directly; writes and
/// processes suspend on the server permission API before execution.
struct ServerTools {
    state: AppState,
}

impl ToolRegistry for ServerTools {
    fn materialize(
        &self,
        _permissions: &[String],
    ) -> Pin<
        Box<
            dyn Future<Output = Option<oc_session_runner::session::services::ToolMaterialization>>
                + Send
                + '_,
        >,
    > {
        let state = self.state.clone();
        Box::pin(async move {
            let mut registry = oc_tool::core::registry::CoreToolRegistry::with_applications();
            let (enable_background_subagents, enable_plan_mode, enable_lsp) = {
                let stores = state.stores.read().await;
                let experimental = stores.config.get("experimental").and_then(Value::as_object);
                let background = experimental
                    .and_then(|experimental| {
                        experimental
                            .get("background_subagents")
                            .or_else(|| experimental.get("backgroundSubagents"))
                    })
                    .and_then(Value::as_bool)
                    .or_else(|| {
                        stores
                            .config
                            .get("experimental_background_subagents")
                            .and_then(Value::as_bool)
                    })
                    .or_else(|| {
                        std::env::var("OPENCODE_EXPERIMENTAL_BACKGROUND_SUBAGENTS")
                            .ok()
                            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
                    })
                    .unwrap_or(false);
                let plan = experimental
                    .and_then(|experimental| {
                        experimental
                            .get("plan_mode")
                            .or_else(|| experimental.get("planMode"))
                    })
                    .and_then(Value::as_bool)
                    .or_else(|| {
                        stores
                            .config
                            .get("experimental_plan_mode")
                            .and_then(Value::as_bool)
                    })
                    .or_else(|| {
                        std::env::var("OPENCODE_EXPERIMENTAL_PLAN_MODE")
                            .ok()
                            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
                    })
                    .unwrap_or(false);
                let lsp_experimental = experimental
                    .and_then(|experimental| {
                        experimental
                            .get("lsp")
                            .or_else(|| experimental.get("lsp_tool"))
                            .or_else(|| experimental.get("lspTool"))
                    })
                    .and_then(Value::as_bool)
                    .or_else(|| {
                        stores
                            .config
                            .get("experimental_lsp_tool")
                            .and_then(Value::as_bool)
                    })
                    .or_else(|| {
                        std::env::var("OPENCODE_EXPERIMENTAL_LSP")
                            .ok()
                            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
                    })
                    .unwrap_or(false);
                (
                    background,
                    plan,
                    lsp_experimental || configured_lsp_exists(&stores.config),
                )
            };
            let registration = registry
                .register(oc_tool::core::builtins_with_lsp_options(
                    false,
                    false,
                    enable_background_subagents,
                    enable_plan_mode,
                    enable_lsp,
                ))
                .ok()?;
            registration();
            let materialization = registry.materialize(&[]);
            let definitions: Vec<oc_session_runner::llm::message::ToolDefinition> = materialization
                .definitions
                .iter()
                .map(|definition| json_convert(definition))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            let mcp_tools = state.mcp_tools.lock().await.clone();
            // MCP resources are first-class read tools in OpenCode.  The
            // instance connection handler owns these live clients (rather
            // than the catalog's separate lifecycle cache), so capture the
            // same server-backed client snapshot used for native MCP tools.
            let mcp_resource_clients = state.mcp_clients.lock().await.clone();
            let plugin_tools = state
                .plugin_reports
                .lock()
                .expect("plugin report lock poisoned")
                .iter()
                .filter_map(|report| report.summary.as_ref())
                .flat_map(|summary| summary.tools.iter().cloned())
                .map(|tool| (tool.name.clone(), tool))
                .collect::<HashMap<_, _>>();
            let mut definitions = definitions;
            for (name, tool) in &mcp_tools {
                if definitions
                    .iter()
                    .any(|definition| definition.name == *name)
                {
                    continue;
                }
                definitions.push(oc_session_runner::llm::message::ToolDefinition {
                    name: name.clone(),
                    description: tool
                        .definition
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("MCP tool {}", tool.definition.name)),
                    input_schema: oc_mcp::catalog::convert_input_schema(&tool.definition),
                    output_schema: tool.definition.output_schema.clone(),
                    cache: None,
                    metadata: None,
                    native: None,
                });
            }
            if mcp_resources_available(&mcp_resource_clients).await {
                definitions.extend(mcp_resource_definitions());
            }
            for (name, tool) in &plugin_tools {
                if definitions
                    .iter()
                    .any(|definition| definition.name == *name)
                {
                    continue;
                }
                definitions.push(oc_session_runner::llm::message::ToolDefinition {
                    name: name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.schema.clone(),
                    output_schema: None,
                    cache: None,
                    metadata: None,
                    native: None,
                });
            }
            let core_settle = Arc::new(CoreToolSettle {
                materialization: Arc::new(materialization),
                state: state.clone(),
            });
            Some(ToolMaterialization {
                definitions,
                settle: Arc::new(ServerToolSettle {
                    core: core_settle,
                    mcp_tools: Arc::new(mcp_tools),
                    mcp_resource_clients: Arc::new(mcp_resource_clients),
                    plugin_manager: state.plugin_manager.clone(),
                    plugin_tools: Arc::new(plugin_tools),
                }),
            })
        })
    }
}

/// Dispatches the materialized local registry and the live MCP catalog from
/// one runner-facing settlement hook.
struct ServerToolSettle {
    core: Arc<CoreToolSettle>,
    mcp_tools: Arc<HashMap<String, crate::state::McpRuntimeTool>>,
    mcp_resource_clients: Arc<HashMap<String, Arc<oc_mcp::client::Client>>>,
    plugin_manager: Option<Arc<oc_plugin::PluginManager>>,
    plugin_tools: Arc<HashMap<String, oc_plugin::ToolInfo>>,
}

impl ToolSettle for ServerToolSettle {
    fn settle(
        &self,
        input: oc_session_runner::session::services::ExecuteInput,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        oc_session_runner::session::services::Settlement,
                        ToolSettlementError,
                    >,
                > + Send
                + '_,
        >,
    > {
        if input.call.name == "question" {
            let state = self.core.state.clone();
            return Box::pin(async move { settle_question(state, input).await });
        }
        if input.call.name == oc_tool::core::plan::NAME {
            let state = self.core.state.clone();
            return Box::pin(async move { settle_plan_exit(state, input).await });
        }
        if is_mcp_resource_tool(&input.call.name) {
            let state = self.core.state.clone();
            let clients = Arc::clone(&self.mcp_resource_clients);
            return Box::pin(async move { settle_mcp_resource_tool(state, clients, input).await });
        }
        if let Some(tool) = self.mcp_tools.get(&input.call.name).cloned() {
            let state = self.core.state.clone();
            return Box::pin(async move {
                let server_name = tool.server.clone();
                let tool_name = tool.definition.name.clone();
                let resource = format!("{server_name}:{tool_name}");
                if !permission_gate(
                    &state,
                    &input.session_id,
                    "mcp",
                    &resource,
                    true,
                    json!({
                        "server": server_name,
                        "tool": tool_name,
                        "callID": input.call.id,
                    }),
                )
                .await
                {
                    return Err(ToolSettlementError::Declined);
                }
                let result = oc_mcp::catalog::call_tool_adapted(
                    tool.client,
                    &tool.definition,
                    input.call.input,
                    tool.timeout,
                )
                .await
                .map_err(|error| ToolSettlementError::Failed(error.to_string()))?;
                let content = result
                    .content
                    .into_iter()
                    .filter_map(|block| match block.r#type.as_str() {
                        "text" => block.text.map(oc_session_runner::llm::ToolContent::text),
                        "resource" => block.resource.and_then(|resource| {
                            resource.text.map(oc_session_runner::llm::ToolContent::text)
                        }),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let structured = result.structured_content.unwrap_or_else(|| Value::Null);
                let tool_result = if content.is_empty() {
                    oc_session_runner::llm::ToolResultValue::Json {
                        value: structured.clone(),
                    }
                } else {
                    oc_session_runner::llm::ToolResultValue::Content {
                        value: content.clone(),
                    }
                };
                Ok(oc_session_runner::session::services::Settlement {
                    result: tool_result,
                    output: Some(oc_session_runner::llm::ToolOutput {
                        structured,
                        content,
                    }),
                    output_paths: Vec::new(),
                })
            });
        }
        if let Some(manager) = self.plugin_manager.clone() {
            if self.plugin_tools.contains_key(&input.call.name) {
                let state = self.core.state.clone();
                let tool_name = input.call.name.clone();
                return Box::pin(async move {
                    settle_plugin_tool(state, manager, tool_name, input).await
                });
            }
        }
        self.core.settle(input)
    }
}

fn is_mcp_resource_tool(name: &str) -> bool {
    matches!(
        name,
        oc_session::tools::MCP_RESOURCE_TOOLS_LIST
            | oc_session::tools::MCP_RESOURCE_TOOLS_LIST_TEMPLATES
            | oc_session::tools::MCP_RESOURCE_TOOLS_READ
    )
}

fn mcp_resource_definitions() -> Vec<oc_session_runner::llm::message::ToolDefinition> {
    vec![
        oc_session_runner::llm::message::ToolDefinition {
            name: oc_session::tools::MCP_RESOURCE_TOOLS_LIST.to_string(),
            description: "List resources exposed by connected MCP servers. Optionally limit the result to one server.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "server": { "type": "string" } },
                "additionalProperties": false,
            }),
            output_schema: None,
            cache: None,
            metadata: None,
            native: None,
        },
        oc_session_runner::llm::message::ToolDefinition {
            name: oc_session::tools::MCP_RESOURCE_TOOLS_LIST_TEMPLATES.to_string(),
            description: "List resource URI templates exposed by connected MCP servers. Optionally limit the result to one server.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "server": { "type": "string" } },
                "additionalProperties": false,
            }),
            output_schema: None,
            cache: None,
            metadata: None,
            native: None,
        },
        oc_session_runner::llm::message::ToolDefinition {
            name: oc_session::tools::MCP_RESOURCE_TOOLS_READ.to_string(),
            description: "Read a resource from a connected MCP server by server name and URI.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "uri": { "type": "string" },
                },
                "required": ["server", "uri"],
                "additionalProperties": false,
            }),
            output_schema: None,
            cache: None,
            metadata: None,
            native: None,
        },
    ]
}

async fn mcp_resources_available(clients: &HashMap<String, Arc<oc_mcp::client::Client>>) -> bool {
    for client in clients.values() {
        if client
            .get_server_capabilities()
            .await
            .map(|capabilities| capabilities.has_resources())
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

async fn settle_mcp_resource_tool(
    state: AppState,
    clients: Arc<HashMap<String, Arc<oc_mcp::client::Client>>>,
    input: oc_session_runner::session::services::ExecuteInput,
) -> Result<oc_session_runner::session::services::Settlement, ToolSettlementError> {
    match input.call.name.as_str() {
        oc_session::tools::MCP_RESOURCE_TOOLS_LIST
        | oc_session::tools::MCP_RESOURCE_TOOLS_LIST_TEMPLATES => {
            let server = oc_session::tools::parse_list_mcp_resources_args(&input.call.input)
                .map_err(ToolSettlementError::Failed)?;
            let targets = mcp_resource_targets(&clients, server.as_deref())?;
            let pattern = server
                .as_deref()
                .map(|server| format!("mcp:{server}:*"))
                .unwrap_or_else(|| "mcp:*".to_string());
            if !permission_gate(
                &state,
                &input.session_id,
                "read",
                &pattern,
                false,
                json!({ "tool": input.call.name, "server": server }),
            )
            .await
            {
                return Err(ToolSettlementError::Declined);
            }

            let mut items = Vec::new();
            for (server, client) in targets {
                let listed: Vec<Value> =
                    if input.call.name == oc_session::tools::MCP_RESOURCE_TOOLS_LIST {
                        oc_mcp::catalog::resources(client, None)
                            .await
                            .map_err(|error| ToolSettlementError::Failed(error.to_string()))?
                            .into_iter()
                            .map(|item| {
                                serde_json::to_value(item)
                                    .map_err(|error| ToolSettlementError::Failed(error.to_string()))
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        oc_mcp::catalog::resource_templates(client, None)
                            .await
                            .map_err(|error| ToolSettlementError::Failed(error.to_string()))?
                            .into_iter()
                            .map(|item| {
                                serde_json::to_value(item)
                                    .map_err(|error| ToolSettlementError::Failed(error.to_string()))
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    };
                for item in listed {
                    let formatted = if input.call.name == oc_session::tools::MCP_RESOURCE_TOOLS_LIST
                    {
                        oc_session::tools::format_mcp_resource(
                            &oc_session::tools::to_record(&item),
                            &server,
                        )
                    } else {
                        oc_session::tools::format_mcp_resource_template(
                            &oc_session::tools::to_record(&item),
                            &server,
                        )
                    };
                    items.push(Value::Object(formatted.into_iter().collect()));
                }
            }
            mcp_resource_json_settlement(Value::Array(items))
        }
        oc_session::tools::MCP_RESOURCE_TOOLS_READ => {
            let (server, uri) = oc_session::tools::parse_read_mcp_resource_args(&input.call.input)
                .map_err(ToolSettlementError::Failed)?;
            let client = clients.get(&server).cloned().ok_or_else(|| {
                ToolSettlementError::Failed(format!("MCP server `{server}` is not connected"))
            })?;
            if !permission_gate(
                &state,
                &input.session_id,
                "read",
                &format!("mcp:{server}:{uri}"),
                false,
                json!({ "tool": input.call.name, "server": server, "uri": uri }),
            )
            .await
            {
                return Err(ToolSettlementError::Declined);
            }
            let result = client
                .read_resource(&uri, oc_mcp::catalog::DEFAULT_REQUEST_TIMEOUT)
                .await
                .map_err(|error| ToolSettlementError::Failed(error.to_string()))?;
            let mut structured_object = serde_json::Map::new();
            structured_object.insert(
                "contents".into(),
                serde_json::to_value(&result.contents)
                    .map_err(|error| ToolSettlementError::Failed(error.to_string()))?,
            );
            if let Some(meta) = result.meta {
                structured_object.insert("_meta".into(), meta);
            }
            structured_object.extend(result.extra);
            let structured = Value::Object(structured_object);
            let formatted =
                oc_session::tools::format_mcp_resource_content(&server, &uri, &structured);
            let mut content = vec![oc_session_runner::llm::ToolContent::text(formatted.text)];
            content.extend(formatted.attachments.into_iter().map(|file| {
                oc_session_runner::llm::ToolContent::File {
                    uri: file.url,
                    mime: file.mime,
                    name: file.filename,
                }
            }));
            Ok(oc_session_runner::session::services::Settlement {
                result: oc_session_runner::llm::ToolResultValue::Content {
                    value: content.clone(),
                },
                output: Some(oc_session_runner::llm::ToolOutput {
                    structured,
                    content,
                }),
                output_paths: Vec::new(),
            })
        }
        _ => Err(ToolSettlementError::Failed(
            "unknown MCP resource tool".into(),
        )),
    }
}

fn mcp_resource_targets(
    clients: &HashMap<String, Arc<oc_mcp::client::Client>>,
    server: Option<&str>,
) -> Result<Vec<(String, Arc<oc_mcp::client::Client>)>, ToolSettlementError> {
    match server {
        Some(server) => clients
            .get(server)
            .cloned()
            .map(|client| vec![(server.to_string(), client)])
            .ok_or_else(|| {
                ToolSettlementError::Failed(format!("MCP server `{server}` is not connected"))
            }),
        None => Ok(clients
            .iter()
            .map(|(server, client)| (server.clone(), client.clone()))
            .collect()),
    }
}

fn mcp_resource_json_settlement(
    value: Value,
) -> Result<oc_session_runner::session::services::Settlement, ToolSettlementError> {
    Ok(oc_session_runner::session::services::Settlement {
        result: oc_session_runner::llm::ToolResultValue::Json {
            value: value.clone(),
        },
        output: Some(oc_session_runner::llm::ToolOutput {
            structured: value,
            content: Vec::new(),
        }),
        output_paths: Vec::new(),
    })
}

async fn settle_plugin_tool(
    state: AppState,
    manager: Arc<oc_plugin::PluginManager>,
    tool_name: String,
    input: oc_session_runner::session::services::ExecuteInput,
) -> Result<oc_session_runner::session::services::Settlement, ToolSettlementError> {
    let location = state
        .stores
        .read()
        .await
        .sessions
        .get(&input.session_id)
        .map(|record| record.info.location.directory.clone())
        .unwrap_or_else(|| state.location.directory.clone());
    if !permission_gate(
        &state,
        &input.session_id,
        "plugin",
        &tool_name,
        true,
        json!({ "tool": tool_name, "callID": input.call.id }),
    )
    .await
    {
        return Err(ToolSettlementError::Declined);
    }
    let context = json!({
        "sessionID": input.session_id,
        "messageID": input.assistant_message_id,
        "agent": input.agent,
        "directory": location,
        "worktree": location,
        "callID": input.call.id,
    });
    let args = input.call.input;
    let cancellation = oc_plugin::PluginToolCancellation::new();
    let cancellation_for_call = cancellation.clone();
    let call = tokio::task::spawn_blocking(move || {
        manager.execute_tool_with_cancellation(tool_name, args, context, cancellation_for_call)
    });
    let session_token = state.session_run_token(&input.session_id).await;
    if session_token
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        cancellation.cancel();
        let _ = call.await;
        return Err(ToolSettlementError::Interrupted);
    }
    // The session runner also has an outer cancellation select. Keep this
    // watcher independent of the settlement future so dropping that future
    // still signals the QuickJS tool instead of leaving the owner thread in
    // an unbounded async loop.
    let cancellation_watcher = session_token.clone().map(|token| {
        let cancellation = cancellation.clone();
        tokio::spawn(async move {
            token.cancelled().await;
            cancellation.cancel();
        })
    });
    let value = if let Some(token) = session_token {
        if token.is_cancelled() {
            cancellation.cancel();
            let _ = call.await;
            return Err(ToolSettlementError::Interrupted);
        }
        tokio::pin!(call);
        tokio::select! {
            biased;
            _ = token.cancelled() => {
                cancellation.cancel();
                let _ = (&mut call).await;
                Err(ToolSettlementError::Interrupted)
            }
            result = &mut call => match result {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(error)) => Err(ToolSettlementError::Failed(error)),
                Err(error) => Err(ToolSettlementError::Failed(error.to_string())),
            },
        }
    } else {
        match call.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(ToolSettlementError::Failed(error)),
            Err(error) => Err(ToolSettlementError::Failed(error.to_string())),
        }
    };
    if let Some(watcher) = cancellation_watcher {
        watcher.abort();
    }
    let value = value?;
    Ok(oc_session_runner::session::services::Settlement {
        result: oc_session_runner::llm::ToolResultValue::Json {
            value: value.clone(),
        },
        output: Some(oc_session_runner::llm::ToolOutput {
            structured: value,
            content: Vec::new(),
        }),
        output_paths: Vec::new(),
    })
}

async fn settle_question(
    state: AppState,
    input: oc_session_runner::session::services::ExecuteInput,
) -> Result<oc_session_runner::session::services::Settlement, ToolSettlementError> {
    let questions_value = input
        .call
        .input
        .get("questions")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let questions: Vec<oc_command::question::Info> =
        serde_json::from_value(questions_value.clone()).map_err(|error| {
            ToolSettlementError::Failed(format!("invalid question input: {error}"))
        })?;
    let tool = oc_command::question::Tool {
        message_id: input.assistant_message_id.clone(),
        call_id: input.call.id.clone(),
    };
    let (handle, request) = state
        .question_service
        .ask(&input.session_id, questions, Some(tool));
    let request_id = request.id.to_string();
    let request_value = serde_json::json!({
        "id": request_id.clone(),
        "sessionID": request.session_id,
        "questions": request.questions,
        "tool": request.tool,
    });
    state
        .stores
        .write()
        .await
        .questions
        .insert(request_id.clone(), request_value.clone());
    state.emit_event(Event {
        id: event_id(),
        metadata: None,
        r#type: "question.asked".into(),
        durable: None,
        location: None,
        data: request_value,
    });
    let answers = handle
        .await
        .map_err(|error| ToolSettlementError::Failed(error.to_string()))?;
    state.stores.write().await.questions.remove(&request_id);
    let answers_value = serde_json::to_value(&answers)
        .map_err(|error| ToolSettlementError::Failed(error.to_string()))?;
    state.emit_event(Event {
        id: event_id(),
        metadata: None,
        r#type: "question.replied".into(),
        durable: None,
        location: None,
        data: serde_json::json!({
            "sessionID": input.session_id,
            "requestID": request_id,
            "answers": answers_value,
        }),
    });
    let text = oc_tool::tool::question::to_model_output(&questions_value, &answers_value);
    Ok(oc_session_runner::session::services::Settlement {
        result: oc_session_runner::llm::ToolResultValue::Text {
            value: Value::String(text.clone()),
        },
        output: Some(oc_session_runner::llm::ToolOutput {
            structured: serde_json::json!({ "answers": answers_value }),
            content: vec![oc_session_runner::llm::ToolContent::text(text)],
        }),
        output_paths: Vec::new(),
    })
}

async fn settle_plan_exit(
    state: AppState,
    input: oc_session_runner::session::services::ExecuteInput,
) -> Result<oc_session_runner::session::services::Settlement, ToolSettlementError> {
    let question_input = oc_session_runner::session::services::ExecuteInput {
        session_id: input.session_id.clone(),
        agent: input.agent.clone(),
        assistant_message_id: input.assistant_message_id.clone(),
        call: oc_session_runner::session::services::ToolCall {
            id: input.call.id.clone(),
            name: "question".to_string(),
            input: serde_json::json!({
                "questions": [{
                    "question": "Plan at PLAN.md is complete. Would you like to switch to the build agent and start implementing?",
                    "header": "Build Agent",
                    "custom": false,
                    "options": [
                        { "label": "Yes", "description": "Switch to build agent and start implementing the plan" },
                        { "label": "No", "description": "Stay with plan agent to continue refining the plan" }
                    ]
                }]
            }),
            provider_executed: false,
            provider_metadata: None,
        },
    };
    let question = settle_question(state.clone(), question_input).await?;
    let answer = question
        .output
        .as_ref()
        .and_then(|output| output.structured.get("answers"))
        .and_then(Value::as_array)
        .and_then(|answers| answers.first())
        .and_then(Value::as_array)
        .and_then(|answers| answers.first())
        .and_then(Value::as_str);
    if answer != Some("Yes") {
        return Err(ToolSettlementError::Failed(
            "Plan exit was not approved; remain in plan mode".to_string(),
        ));
    }

    let info = {
        let mut stores = state.stores.write().await;
        let record = stores
            .sessions
            .get_mut(&input.session_id)
            .ok_or_else(|| ToolSettlementError::Failed("Session not found".to_string()))?;
        record.info.agent = Some("build".to_string());
        record.info.time.updated = now_millis();
        record.info.clone()
    };
    state.persist_session(&info);
    state.emit_event(Event {
        id: event_id(),
        metadata: None,
        r#type: "session.updated".into(),
        durable: None,
        location: None,
        data: json!({ "sessionID": input.session_id, "info": info }),
    });

    let text = "User approved switching to build agent. Wait for further instructions.".to_string();
    Ok(oc_session_runner::session::services::Settlement {
        result: oc_session_runner::llm::ToolResultValue::Text {
            value: Value::String(text.clone()),
        },
        output: Some(oc_session_runner::llm::ToolOutput {
            structured: json!({ "status": "approved", "agent": "build" }),
            content: vec![oc_session_runner::llm::ToolContent::text(text)],
        }),
        output_paths: Vec::new(),
    })
}

struct CoreToolSettle {
    materialization: Arc<oc_tool::core::registry::Materialization>,
    state: AppState,
}

impl ToolSettle for CoreToolSettle {
    fn settle(
        &self,
        input: oc_session_runner::session::services::ExecuteInput,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        oc_session_runner::session::services::Settlement,
                        ToolSettlementError,
                    >,
                > + Send
                + '_,
        >,
    > {
        let materialization = self.materialization.clone();
        let state = self.state.clone();
        Box::pin(async move {
            let (subagent_depth, subagent_parent_depth) = {
                let stores = state.stores.read().await;
                (
                    stores
                        .config
                        .get("subagent_depth")
                        .and_then(Value::as_u64)
                        .map(|depth| depth as usize),
                    session_parent_depth(&stores.sessions, &input.session_id),
                )
            };
            let location = state
                .stores
                .read()
                .await
                .sessions
                .get(&input.session_id)
                .map(|record| record.info.location.directory.clone())
                .unwrap_or_else(|| state.location.directory.clone());
            if !authorize_tool(
                &state,
                &input.session_id,
                &location,
                &input.call.name,
                &input.call.input,
            )
            .await
            {
                return Err(ToolSettlementError::Declined);
            }
            let todo_update = (input.call.name == "todowrite")
                .then(|| input.call.input.get("todos").cloned())
                .flatten();
            let todo_session_id = input.session_id.clone();
            let subagent_executor = server_subagent_executor(state.clone());
            let lsp_request = server_lsp_executor(state.clone(), location.clone());
            let core_input = oc_tool::core::registry::ExecuteInput {
                session_id: input.session_id.clone(),
                agent: input.agent.clone(),
                assistant_message_id: input.assistant_message_id.clone(),
                call: oc_tool::model::ToolCall {
                    id: input.call.id,
                    name: input.call.name,
                    input: input.call.input,
                },
            };
            let result = tokio::task::spawn_blocking(move || {
                let mut core_input = core_input;
                let mut context = oc_tool::core::tool::CoreContext {
                    session_id: core_input.session_id.clone(),
                    agent: core_input.agent.clone(),
                    assistant_message_id: core_input.assistant_message_id.clone(),
                    tool_call_id: core_input.call.id.clone(),
                    location_directory: location,
                    asks: Vec::new(),
                    subagent_depth,
                    subagent_parent_depth: Arc::new(move |_| subagent_parent_depth),
                    execute_subagent: Some(subagent_executor),
                    lsp_request: Some(lsp_request),
                };
                (materialization.settle)(&mut core_input, &mut context)
            })
            .await
            .map_err(|error| ToolSettlementError::Failed(error.to_string()))?;
            if let Some(todos) = todo_update {
                if let Ok(mut stores) = state.stores.try_write() {
                    stores.todos.insert(todo_session_id, todos);
                }
            }
            match result {
                oc_tool::core::registry::Settlement::Ok {
                    result,
                    output,
                    output_paths,
                } => Ok(oc_session_runner::session::services::Settlement {
                    result: core_result(result),
                    output: output.map(core_output),
                    output_paths,
                }),
                oc_tool::core::registry::Settlement::Error { value } => {
                    Ok(oc_session_runner::session::services::Settlement {
                        result: oc_session_runner::llm::ToolResultValue::Error {
                            value: Value::String(value),
                        },
                        output: None,
                        output_paths: Vec::new(),
                    })
                }
            }
        })
    }
}

#[derive(Debug, Clone)]
struct ResolvedLspServer {
    id: String,
    command: Vec<String>,
    initialization: Option<Value>,
}

fn configured_lsp_exists(config: &Value) -> bool {
    config
        .get("lsp")
        .and_then(Value::as_object)
        .map(|servers| {
            servers.values().any(|entry| {
                entry.get("disabled").and_then(Value::as_bool) != Some(true)
                    && entry
                        .get("command")
                        .and_then(Value::as_array)
                        .map(|command| !command.is_empty())
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn resolve_lsp_server(config: &Value, file: &Path) -> Option<ResolvedLspServer> {
    let extension = file.extension()?.to_string_lossy().to_ascii_lowercase();
    let servers = config.get("lsp")?.as_object()?;
    for (id, entry) in servers {
        if entry.get("disabled").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(command) = entry
            .get("command")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|command| !command.is_empty())
        else {
            continue;
        };
        let extensions = entry
            .get("extensions")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let matches = extensions.iter().any(|value| {
            let value = value.trim().trim_start_matches('.').to_ascii_lowercase();
            value == extension || value == format!(".{extension}")
        });
        if !matches {
            continue;
        }
        return Some(ResolvedLspServer {
            id: id.clone(),
            command,
            initialization: entry.get("initialization").cloned(),
        });
    }
    None
}

fn server_lsp_executor(state: AppState, root: String) -> oc_tool::core::tool::CoreLspRequest {
    Arc::new(move |request| {
        let state = state.clone();
        let root = root.clone();
        oc_tool::core::tool::run_future(Box::pin(async move {
            execute_lsp_request(state, root, request).await
        }))
    })
}

async fn execute_lsp_request(
    state: AppState,
    root: String,
    request: oc_tool::model::LspRequest,
) -> Result<Vec<Value>, String> {
    let file = PathBuf::from(&request.file_path);
    let root_path = std::fs::canonicalize(&root).unwrap_or_else(|_| PathBuf::from(&root));
    let file = std::fs::canonicalize(&file)
        .map_err(|error| format!("LSP file is not readable `{}`: {error}", file.display()))?;
    if !oc_tool::util::fs_contains(&root_path.to_string_lossy(), &file.to_string_lossy()) {
        return Err(format!(
            "LSP file must be inside the workspace: {}",
            file.display()
        ));
    }
    let config = state.stores.read().await.config.clone();
    let server = resolve_lsp_server(&config, &file).ok_or_else(|| {
        let extension = file
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown");
        format!("No LSP server configured for file extension `.{extension}`")
    })?;
    let operation = match request.operation.as_str() {
        "goToDefinition" => oc_project::lsp::LspOperation::GoToDefinition,
        "findReferences" => oc_project::lsp::LspOperation::FindReferences,
        "hover" => oc_project::lsp::LspOperation::Hover,
        "documentSymbol" => oc_project::lsp::LspOperation::DocumentSymbol,
        "workspaceSymbol" => oc_project::lsp::LspOperation::WorkspaceSymbol,
        "goToImplementation" => oc_project::lsp::LspOperation::GoToImplementation,
        "prepareCallHierarchy" => oc_project::lsp::LspOperation::PrepareCallHierarchy,
        "incomingCalls" => oc_project::lsp::LspOperation::IncomingCalls,
        "outgoingCalls" => oc_project::lsp::LspOperation::OutgoingCalls,
        other => return Err(format!("Unsupported LSP operation `{other}`")),
    };
    let key = serde_json::to_string(&(root_path.to_string_lossy(), &server.id, &server.command))
        .unwrap_or_else(|_| format!("{}:{}", root_path.display(), server.id));
    let adapter = if let Some(adapter) = state.lsp_adapters.lock().await.get(&key).cloned() {
        adapter
    } else {
        let mut process = oc_project::lsp::LspServerConfig::new(&server.command[0]);
        process.args = server.command[1..].to_vec();
        process.cwd = Some(root_path.clone());
        process.initialization_options = server.initialization.clone();
        let adapter = Arc::new(
            oc_project::lsp::LspAdapter::start(process, root_path.clone())
                .await
                .map_err(|error| error.to_string())?,
        );
        let mut adapters = state.lsp_adapters.lock().await;
        adapters
            .entry(key)
            .or_insert_with(|| adapter.clone())
            .clone()
    };
    let response = adapter
        .request_operation(
            operation,
            &file,
            request.line,
            request.character,
            request.query.as_deref(),
        )
        .await
        .map_err(|error| format!("LSP {} failed: {error}", request.operation))?;
    Ok(match response {
        Value::Array(values) => values,
        Value::Null => Vec::new(),
        value => vec![value],
    })
}

/// Run one foreground child session through the same durable runner used by
/// normal prompts. Background mode remains rejected by the core task schema;
/// this callback therefore has a single ownership path and cannot fabricate a
/// completed result when the child provider produces no assistant output.
fn server_subagent_executor(state: AppState) -> oc_tool::core::tool::CoreSubagentExecute {
    Arc::new(move |request| {
        let state = state.clone();
        Box::pin(async move { execute_subagent(state, request).await })
    })
}

async fn execute_subagent(
    state: AppState,
    request: oc_tool::model::SubagentRequest,
) -> Result<oc_tool::model::SubagentResult, String> {
    if state
        .session_run_token(&request.parent_session_id)
        .await
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return Err(format!(
            "subagent cancelled because parent session `{}` was aborted",
            request.parent_session_id
        ));
    }

    let child_id = request
        .task_id
        .clone()
        .unwrap_or_else(crate::event::session_id);

    // A task id supplied by the caller is a resume request. Re-admit the new
    // prompt into the durable child session instead of fabricating a second
    // session with the same identity.
    if request.task_id.is_some() && state.stores.read().await.sessions.contains_key(&child_id) {
        let (info, message) = {
            let mut stores = state.stores.write().await;
            let record = stores
                .sessions
                .get_mut(&child_id)
                .ok_or_else(|| format!("subagent session `{child_id}` disappeared"))?;
            if record.info.parent_id.as_deref() != Some(request.parent_session_id.as_str()) {
                return Err(format!(
                    "subagent session `{child_id}` is not a child of `{}`",
                    request.parent_session_id
                ));
            }
            let created = now_millis();
            let message = serde_json::json!({
                "id": session_message_id(),
                "time": { "created": created },
                "type": "user",
                "text": request.prompt,
                "files": [],
                "agents": [],
            });
            record.messages.push(message.clone());
            record.info.time.updated = created;
            record.active = true;
            (record.info.clone(), message)
        };
        state.persist_session(&info);
        state.persist_message(&child_id, &message);
        if request.background {
            spawn_background_subagent(
                state,
                child_id.clone(),
                request.parent_session_id.clone(),
                request.description.clone(),
                request.subagent_type.clone(),
            )
            .await;
            return Ok(background_subagent_result(
                child_id,
                request.parent_session_id,
            ));
        }
        run_foreground_subagent(state.clone(), child_id.clone(), &request.parent_session_id)
            .await?;
        return completed_subagent_result(state, child_id, request.parent_session_id).await;
    }

    let (info, message) = {
        let mut stores = state.stores.write().await;
        if stores.sessions.contains_key(&child_id) {
            return Err(format!(
                "cannot resume existing subagent session `{child_id}` yet"
            ));
        }
        let parent = stores
            .sessions
            .get(&request.parent_session_id)
            .ok_or_else(|| {
                format!(
                    "parent session `{}` was not found",
                    request.parent_session_id
                )
            })?
            .info
            .clone();
        let created = now_millis();
        let info = SessionInfo {
            id: child_id.clone(),
            parent_id: Some(request.parent_session_id.clone()),
            project_id: parent.project_id,
            agent: Some(request.subagent_type.clone()),
            model: parent.model,
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created,
                updated: created,
                archived: None,
            },
            title: request.description.clone(),
            location: parent.location,
            subpath: None,
            revert: None,
        };
        let message = serde_json::json!({
            "id": session_message_id(),
            "time": { "created": created },
            "type": "user",
            "text": request.prompt,
            "files": [],
            "agents": [],
        });
        stores.sessions.insert(
            child_id.clone(),
            crate::state::SessionRecord {
                info: info.clone(),
                messages: vec![message.clone()],
                active: true,
            },
        );
        (info, message)
    };
    state.persist_session(&info);
    state.persist_message(&child_id, &message);

    if request.background {
        spawn_background_subagent(
            state,
            child_id.clone(),
            request.parent_session_id.clone(),
            request.description.clone(),
            request.subagent_type.clone(),
        )
        .await;
        return Ok(background_subagent_result(
            child_id,
            request.parent_session_id,
        ));
    }

    run_foreground_subagent(state.clone(), child_id.clone(), &request.parent_session_id).await?;

    completed_subagent_result(state, child_id, request.parent_session_id).await
}

/// Run a foreground child while inheriting cancellation from its parent.
///
/// The core task callback is intentionally host-agnostic, so the server owns
/// this bridge. A parent abort must not merely interrupt the parent's tool
/// fiber: it must also stop the child runner and leave the durable child
/// session inactive.
async fn run_foreground_subagent(
    state: AppState,
    child_id: String,
    parent_session_id: &str,
) -> Result<(), String> {
    let parent_token = state.session_run_token(parent_session_id).await;
    if parent_token
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return Err(format!(
            "subagent cancelled because parent session `{parent_session_id}` was aborted"
        ));
    }

    let child_token = CancellationToken::new();
    let child_run = run_session_with_token(state.clone(), child_id.clone(), child_token.clone());
    tokio::pin!(child_run);

    if let Some(parent_token) = parent_token {
        tokio::select! {
            _ = parent_token.cancelled() => {
                child_token.cancel();
                child_run.await;
                set_active(&state, &child_id, false).await;
                Err(format!(
                    "subagent cancelled because parent session `{parent_session_id}` was aborted"
                ))
            }
            _ = &mut child_run => Ok(()),
        }
    } else {
        child_run.await;
        Ok(())
    }
}

fn background_subagent_result(
    child_id: String,
    parent_session_id: String,
) -> oc_tool::model::SubagentResult {
    oc_tool::model::SubagentResult {
        session_id: child_id,
        state: "running".into(),
        summary: Some("Background subagent started".into()),
        output: String::new(),
        metadata: json!({ "parentSessionId": parent_session_id }),
    }
}

async fn spawn_background_subagent(
    state: AppState,
    child_id: String,
    parent_session_id: String,
    title: String,
    agent: String,
) {
    let run_state = state.clone();
    let run_child_id = child_id.clone();
    let run_parent_id = parent_session_id.clone();
    let run: oc_core::background_job::Run = Arc::new(move || {
        let state = run_state.clone();
        let child_id = run_child_id.clone();
        let parent_session_id = run_parent_id.clone();
        Box::pin(async move {
            // A cancellation can race with the registry's task spawn. Avoid
            // admitting a runner after the job has already been cancelled.
            if state
                .background_jobs
                .get(&child_id)
                .await
                .is_some_and(|info| info.status != "running")
            {
                return Err("background job cancelled before execution".into());
            }

            let Some(token) = state.acquire_session_run(&child_id).await else {
                return Err(format!(
                    "background session `{child_id}` is already running"
                ));
            };
            run_session_with_token(state.clone(), child_id.clone(), token).await;
            let _ = state.finish_session_run(&child_id).await;

            let status = state
                .background_jobs
                .get(&child_id)
                .await
                .map(|info| info.status)
                .unwrap_or_else(|| "completed".into());
            state.emit_event(Event {
                id: event_id(),
                metadata: None,
                r#type: "session.updated".into(),
                durable: None,
                location: None,
                data: json!({
                    "sessionID": child_id,
                    "parentID": parent_session_id,
                    "status": status
                }),
            });

            if status == "cancelled" {
                return Err("background job cancelled".into());
            }
            completed_subagent_result(state, child_id, parent_session_id)
                .await
                .map(|result| result.output)
        })
    });

    let _ = state
        .background_jobs
        .start(oc_core::background_job::StartInput {
            id: Some(child_id),
            r#type: "subagent".into(),
            title: Some(title),
            metadata: Some(
                serde_json::json!({
                    "parentSessionID": parent_session_id,
                    "agent": agent,
                    "background": true,
                })
                .as_object()
                .expect("background metadata object")
                .clone(),
            ),
            on_promote: None,
            run,
        })
        .await;
}

async fn completed_subagent_result(
    state: AppState,
    child_id: String,
    parent_session_id: String,
) -> Result<oc_tool::model::SubagentResult, String> {
    let output = {
        let stores = state.stores.read().await;
        let record = stores
            .sessions
            .get(&child_id)
            .ok_or_else(|| format!("subagent session `{child_id}` disappeared"))?;
        record.messages.iter().rev().find_map(|message| {
            if message.get("type").and_then(Value::as_str) != Some("assistant") {
                return None;
            }
            message
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    message
                        .get("content")
                        .and_then(Value::as_array)
                        .map(|parts| {
                            parts
                                .iter()
                                .filter_map(|part| part.get("text").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join("")
                        })
                })
        })
    }
    .filter(|text| !text.is_empty())
    .ok_or_else(|| format!("subagent session `{child_id}` completed without assistant output"))?;

    Ok(oc_tool::model::SubagentResult {
        session_id: child_id,
        state: "completed".into(),
        summary: Some("Foreground subagent completed".into()),
        output,
        metadata: json!({ "parentSessionId": parent_session_id }),
    })
}

fn core_result(result: oc_tool::model::ToolResultValue) -> oc_session_runner::llm::ToolResultValue {
    match result {
        oc_tool::model::ToolResultValue::Json { value } => {
            oc_session_runner::llm::ToolResultValue::Json { value }
        }
        oc_tool::model::ToolResultValue::Text { value } => {
            oc_session_runner::llm::ToolResultValue::Text { value }
        }
        oc_tool::model::ToolResultValue::Error { value } => {
            oc_session_runner::llm::ToolResultValue::Error { value }
        }
        oc_tool::model::ToolResultValue::Content { value } => {
            oc_session_runner::llm::ToolResultValue::Content {
                value: value.into_iter().map(core_content).collect(),
            }
        }
    }
}

fn core_output(output: oc_tool::model::ToolOutput) -> oc_session_runner::llm::ToolOutput {
    oc_session_runner::llm::ToolOutput {
        structured: output.structured,
        content: output.content.into_iter().map(core_content).collect(),
    }
}

fn core_content(content: oc_tool::model::ToolContent) -> oc_session_runner::llm::ToolContent {
    match content {
        oc_tool::model::ToolContent::Text { text } => {
            oc_session_runner::llm::ToolContent::Text { text }
        }
        oc_tool::model::ToolContent::File { uri, mime, name } => {
            oc_session_runner::llm::ToolContent::File { uri, mime, name }
        }
    }
}

async fn authorize_tool(
    state: &AppState,
    session_id: &str,
    location: &str,
    name: &str,
    input: &Value,
) -> bool {
    if !matches!(
        name,
        "read"
            | "glob"
            | "grep"
            | "write"
            | "edit"
            | "bash"
            | "todowrite"
            | "webfetch"
            | "websearch"
            | "skill"
            | "apply_patch"
            | "task"
            | "lsp"
    ) {
        return false;
    }
    if name == "glob" {
        let pattern = input.get("pattern").and_then(Value::as_str).unwrap_or("");
        if std::path::Path::new(pattern)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return false;
        }
    }
    let path = input
        .get("path")
        .or_else(|| input.get("workdir"))
        .or_else(|| input.get("filePath"))
        .and_then(Value::as_str);
    if let Some(path) = path {
        if !safe_workspace_path(location, path) {
            let allowed = permission_gate(
                state,
                session_id,
                "external_directory",
                path,
                true,
                json!({ "filepath": path }),
            )
            .await;
            if !allowed {
                return false;
            }
        }
    } else if matches!(name, "write" | "edit") {
        return false;
    }
    let (permission, pattern, fallback_ask, metadata) = match name {
        "write" | "edit" | "apply_patch" => {
            ("edit", path.unwrap_or("*"), true, json!({ "tool": name }))
        }
        "bash" => (
            "bash",
            input
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            true,
            json!({ "tool": "bash" }),
        ),
        "task" => (
            "task",
            input
                .get("subagent_type")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            true,
            json!({
                "tool": "task",
                "description": input.get("description").cloned().unwrap_or(Value::Null),
            }),
        ),
        // The reference lsp tool always asks (`patterns: ["*"], always: ["*"]`);
        // configured allow/deny rules still win through `permission_gate`.
        "lsp" => ("lsp", path.unwrap_or("*"), true, json!({ "tool": "lsp" })),
        _ => (name, path.unwrap_or("*"), false, json!({ "tool": name })),
    };
    permission_gate(
        state,
        session_id,
        permission,
        pattern,
        fallback_ask,
        metadata,
    )
    .await
}

/// Apply the configured global/agent permission rules before falling back to
/// the interactive permission service. A missing rule preserves the existing
/// behavior: reads are admitted, while writes/processes ask the connected
/// client. Explicit `allow`, `deny`, and `ask` rules always win.
async fn permission_gate(
    state: &AppState,
    session_id: &str,
    permission: &str,
    pattern: &str,
    fallback_ask: bool,
    metadata: Value,
) -> bool {
    let configured_action = {
        let stores = state.stores.read().await;
        let session = stores.sessions.get(session_id);
        let project_id = session
            .map(|record| record.info.project_id.as_str())
            .unwrap_or("global");
        let saved_action = stores
            .saved_permissions
            .values()
            .find(|saved| {
                saved.project_id == project_id
                    && oc_session::permission::wildcard::matches(pattern, &saved.resource)
                    && saved.action == permission
            })
            .map(|_| "allow".to_string());
        let agent = session
            .and_then(|record| record.info.agent.as_deref())
            .unwrap_or("build");
        let rules = configured_permission_rules(&stores.config, agent);
        saved_action.or_else(|| {
            rules
                .iter()
                .rev()
                .find(|rule| {
                    oc_session::permission::wildcard::matches(permission, &rule.permission)
                        && oc_session::permission::wildcard::matches(pattern, &rule.pattern)
                })
                .map(|rule| rule.action.to_ascii_lowercase())
        })
    };

    match configured_action.as_deref() {
        Some("allow") => true,
        Some("deny") => false,
        Some("ask") => {
            state
                .request_permission(session_id, permission, vec![pattern.to_string()], metadata)
                .await
        }
        _ if fallback_ask => {
            state
                .request_permission(session_id, permission, vec![pattern.to_string()], metadata)
                .await
        }
        _ => true,
    }
}

fn session_parent_depth(sessions: &HashMap<String, SessionRecord>, session_id: &str) -> usize {
    let mut depth = 0usize;
    let mut current = session_id;
    while let Some(parent_id) = sessions
        .get(current)
        .and_then(|record| record.info.parent_id.as_deref())
    {
        depth += 1;
        if depth > 1024 {
            break;
        }
        current = parent_id;
    }
    depth
}

fn safe_workspace_path(location: &str, path: &str) -> bool {
    let resolved = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        std::path::Path::new(location).join(path)
    };
    if !oc_tool::util::fs_contains(location, &resolved.to_string_lossy()) {
        return false;
    }
    let Ok(root) = std::fs::canonicalize(location) else {
        return false;
    };
    match std::fs::canonicalize(&resolved) {
        Ok(real) => real.starts_with(root),
        Err(_) => true,
    }
}

fn runner_session_info(info: &SessionInfo) -> RunnerSessionInfo {
    RunnerSessionInfo {
        id: info.id.clone(),
        agent: info.agent.clone(),
        model: info.model.as_ref().map(runner_model_ref),
        location: RunnerLocationRef {
            directory: info.location.directory.clone(),
            workspace_id: info.location.workspace_id.clone(),
        },
    }
}

fn runner_model_ref(model: &ModelRef) -> RunnerModelRef {
    RunnerModelRef {
        id: model.id.clone(),
        provider_id: model.provider_id.clone(),
        variant: model.variant.clone(),
    }
}

fn server_message(value: &Value) -> Option<SessionMessage> {
    let id = value.get("id")?.as_str()?.to_string();
    let time = MessageTime {
        created: timestamp_string(value),
        completed: value
            .get("time")
            .and_then(|time| time.get("completed"))
            .map(value_string),
    };
    match value
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| value.get("role").and_then(Value::as_str))?
    {
        "user" => Some(SessionMessage::User(User {
            id,
            kind: MessageKind::User,
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            files: value.get("files").and_then(|files| {
                let files = files.as_array()?;
                Some(
                    files
                        .iter()
                        .filter_map(|file| {
                            serde_json::from_value::<FileAttachment>(file.clone()).ok()
                        })
                        .collect(),
                )
            }),
            agents: value.get("agents").and_then(|agents| {
                let agents = agents.as_array()?;
                Some(
                    agents
                        .iter()
                        .filter_map(|agent| {
                            serde_json::from_value::<AgentAttachment>(agent.clone()).ok()
                        })
                        .collect(),
                )
            }),
            metadata: value.get("metadata").and_then(Value::as_object).cloned(),
            time,
        })),
        "assistant" => {
            let model = RunnerModelRef {
                id: value
                    .get("modelID")
                    .or_else(|| value.get("model").and_then(|model| model.get("id")))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                provider_id: value
                    .get("providerID")
                    .or_else(|| value.get("model").and_then(|model| model.get("providerID")))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                variant: None,
            };
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut content = value
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(parse_assistant_content)
                .collect::<Vec<_>>();
            if !text.is_empty()
                && !content
                    .iter()
                    .any(|item| matches!(item, AssistantContent::Text(_)))
            {
                content.insert(
                    0,
                    AssistantContent::Text(AssistantText {
                        kind: AssistantContentKind::Text,
                        id: format!("{id}_text"),
                        text,
                    }),
                );
            }
            Some(SessionMessage::Assistant(Assistant {
                id,
                kind: MessageKind::Assistant,
                agent: value
                    .get("agent")
                    .and_then(Value::as_str)
                    .unwrap_or("build")
                    .to_string(),
                model,
                content,
                error: None,
                snapshot: None,
                finish: value
                    .get("finish")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                cost: value.get("cost").and_then(Value::as_f64),
                tokens: value.get("tokens").map(runner_message_tokens),
                metadata: None,
                time,
            }))
        }
        "system" => Some(SessionMessage::System(System {
            id,
            kind: MessageKind::System,
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            metadata: None,
            time,
        })),
        "compaction" => Some(SessionMessage::Compaction(Compaction {
            id,
            kind: MessageKind::Compaction,
            reason: match value.get("reason").and_then(Value::as_str) {
                Some("auto") => CompactionReason::Auto,
                _ => CompactionReason::Manual,
            },
            summary: value
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            recent: value
                .get("recent")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            metadata: None,
            time,
        })),
        _ => None,
    }
}

fn parse_assistant_content(value: &Value) -> Option<AssistantContent> {
    match value.get("type").and_then(Value::as_str)? {
        "text" => serde_json::from_value::<AssistantText>(value.clone())
            .ok()
            .map(AssistantContent::Text),
        "reasoning" => serde_json::from_value(value.clone())
            .ok()
            .map(AssistantContent::Reasoning),
        "tool" => serde_json::from_value(value.clone())
            .ok()
            .map(AssistantContent::Tool),
        _ => None,
    }
}

fn runner_message_tokens(value: &Value) -> RunnerMessageTokens {
    RunnerMessageTokens {
        input: value.get("input").and_then(Value::as_f64).unwrap_or(0.0),
        output: value.get("output").and_then(Value::as_f64).unwrap_or(0.0),
        reasoning: value
            .get("reasoning")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        cache: oc_session_runner::session::message::CacheTokens {
            read: value
                .get("cache")
                .and_then(|cache| cache.get("read"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            write: value
                .get("cache")
                .and_then(|cache| cache.get("write"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        },
    }
}

fn timestamp_string(value: &Value) -> String {
    value
        .get("time")
        .and_then(|time| time.get("created"))
        .map(value_string)
        .unwrap_or_else(|| now_millis().to_string())
}

fn value_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::auth::AuthConfig;
    use crate::cors::CorsOptions;
    use crate::location::Location;
    use crate::state::SessionRecord;
    use oc_session_runner::llm::event::{ToolContent, ToolResultValue};
    use oc_session_runner::session::event::{CacheTokens, Provider, RetryError, Tokens};
    use oc_session_runner::session::util::timestamp_now;

    #[test]
    fn server_user_message_lowers_to_runner_history() {
        let value = json!({
            "id": "msg_user",
            "type": "user",
            "text": "hello",
            "files": [{
                "uri": "file:///tmp/readme.md",
                "mime": "text/markdown",
                "name": "readme.md"
            }],
            "agents": [{"name": "reviewer"}],
            "metadata": {"source": "tui"},
            "time": { "created": 42 }
        });
        let Some(SessionMessage::User(user)) = server_message(&value) else {
            panic!("expected user message")
        };
        assert_eq!(user.id, "msg_user");
        assert_eq!(user.text, "hello");
        assert_eq!(user.time.created, "42");
        assert_eq!(user.files.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            user.files.as_ref().unwrap()[0].name.as_deref(),
            Some("readme.md")
        );
        assert_eq!(user.agents.as_ref().map(Vec::len), Some(1));
        assert_eq!(user.agents.as_ref().unwrap()[0].name, "reviewer");
        assert_eq!(user.metadata.as_ref().unwrap()["source"], "tui");
    }

    #[tokio::test]
    async fn production_runner_resolves_configured_agent_and_model_defaults() {
        let state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({
                "model": "openai/gpt-4o-mini",
                "agent": {
                    "reviewer": {
                        "model": "anthropic/claude-sonnet-4",
                        "system": "Review the change carefully.",
                        "steps": 4,
                        "permission": { "read": "allow" }
                    }
                }
            }),
        );
        let selection = ServerAgents {
            state: state.clone(),
        }
        .select(Some("reviewer"))
        .await;
        let info = selection.info.expect("configured agent");
        assert_eq!(info.system.as_deref(), Some("Review the change carefully."));
        assert_eq!(info.steps, Some(4));
        assert_eq!(info.permissions, vec!["read"]);

        let session = RunnerSessionInfo {
            id: "ses_configured_model".into(),
            agent: Some("reviewer".into()),
            model: None,
            location: RunnerLocationRef {
                directory: "/tmp".into(),
                workspace_id: None,
            },
        };
        let model = ServerModel { state }.resolve(&session).await.unwrap();
        assert_eq!(model.provider, "anthropic");
        assert_eq!(model.id, "claude-sonnet-4");
    }

    #[tokio::test]
    async fn production_runner_uses_configured_model_cost_and_limits() {
        let state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({
                "provider": {
                    "custom": {
                        "models": {
                            "agent-model": {
                                "cost": {
                                    "input": 2.0,
                                    "output": 5.0,
                                    "cacheRead": 0.5,
                                    "cacheWrite": 1.0
                                },
                                "limit": { "context": 64000, "output": 8192 }
                            }
                        }
                    }
                }
            }),
        );
        let session = RunnerSessionInfo {
            id: "ses_configured_metadata".into(),
            agent: Some("build".into()),
            model: Some(RunnerModelRef {
                id: "agent-model".into(),
                provider_id: "custom".into(),
                variant: None,
            }),
            location: RunnerLocationRef {
                directory: "/tmp".into(),
                workspace_id: None,
            },
        };
        let model = ServerModel { state }.resolve(&session).await.unwrap();
        assert_eq!(model.cost.as_ref().map(|cost| cost.input), Some(2.0));
        assert_eq!(
            model.limits.as_ref().and_then(|limits| limits.context),
            Some(64_000)
        );
        assert_eq!(
            model.limits.as_ref().and_then(|limits| limits.output),
            Some(8_192)
        );
    }

    #[test]
    fn compaction_transcript_preserves_structured_parts() {
        let transcript = compact_message_lines(&[
            json!({
                "type": "user",
                "text": "inspect this",
                "files": [{ "name": "src/main.rs", "mime": "text/x-rust-source" }]
            }),
            json!({
                "type": "assistant",
                "content": [
                    { "type": "reasoning", "text": "check the entrypoint" },
                    { "type": "tool", "name": "read", "input": { "path": "src/main.rs" } }
                ]
            }),
        ]);
        assert!(transcript.contains("src/main.rs"));
        assert!(transcript.contains("check the entrypoint"));
        assert!(transcript.contains("\"name\":\"read\""));
    }

    #[test]
    fn legacy_compaction_marks_old_completed_tool_parts() {
        let tool = |index: usize, name: &str| {
            json!({
                "id": format!("part_{index}"),
                "type": "tool",
                "name": name,
                "state": {
                    "status": "completed",
                    "input": {},
                    "content": [],
                    "structured": {},
                    "result": "x".repeat(39_000)
                },
                "time": { "created": "1", "completed": "2" }
            })
        };
        let mut messages = vec![
            json!({ "id": "u1", "type": "user", "text": "old" }),
            json!({
                "id": "a1",
                "type": "assistant",
                "content": (0..8)
                    .map(|index| tool(index, if index == 2 { "skill" } else { "read" }))
                    .collect::<Vec<_>>()
            }),
            json!({ "id": "u2", "type": "user", "text": "middle" }),
            json!({ "id": "a2", "type": "assistant", "content": [] }),
            json!({ "id": "u3", "type": "user", "text": "recent" }),
            json!({ "id": "a3", "type": "assistant", "content": [] }),
        ];

        let candidates = legacy_prune_candidates(&messages);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["part_3", "part_1", "part_0"]
        );

        let marked = mark_legacy_pruned_parts(&mut messages, &candidates, "123");
        assert_eq!(marked.len(), 3);
        assert_eq!(messages[1]["content"][3]["time"]["pruned"], "123");
        assert_eq!(messages[1]["content"][1]["time"]["pruned"], "123");
        assert_eq!(messages[1]["content"][0]["time"]["pruned"], "123");
        assert!(messages[1]["content"][2]["time"].get("pruned").is_none());
        assert!(messages[1]["content"][7]["time"].get("pruned").is_none());
    }

    #[test]
    fn legacy_compaction_does_not_mark_below_savings_threshold() {
        let mut messages = vec![
            json!({ "id": "u1", "type": "user" }),
            json!({
                "id": "a1",
                "type": "assistant",
                "content": (0..6)
                    .map(|index| json!({
                        "id": format!("part_{index}"),
                        "type": "tool",
                        "name": "read",
                        "state": {
                            "status": "completed",
                            "content": [],
                            "structured": {},
                            "result": "x".repeat(39_000)
                        },
                        "time": { "created": "1", "completed": "2" }
                    }))
                    .collect::<Vec<_>>()
            }),
            json!({ "id": "u2", "type": "user" }),
            json!({ "id": "a2", "type": "assistant", "content": [] }),
            json!({ "id": "u3", "type": "user" }),
            json!({ "id": "a3", "type": "assistant", "content": [] }),
        ];

        assert!(legacy_prune_candidates(&messages).is_empty());
        assert!(mark_legacy_pruned_parts(&mut messages, &[], "123").is_empty());
        assert!(messages[1]["content"]
            .as_array()
            .unwrap()
            .iter()
            .all(|part| part["time"].get("pruned").is_none()));
    }

    #[test]
    fn compaction_trigger_uses_resolved_model_limits() {
        let message = server_message(&json!({
            "id": "msg_model_limits",
            "type": "user",
            "text": "keep this context"
        }))
        .expect("user message");
        let model = RunnerLlmModel::make("bounded", "stub").with_limits(RunnerModelLimits {
            context: Some(100_000),
            input: None,
            output: Some(10_000),
        });
        let input = compaction_test_input("ses_model_limits", model, message);

        assert!(!compaction_needed(&input, 8_000));
        assert!(compaction_needed(&input, 360_000));
    }

    #[test]
    fn auto_compaction_respects_config_disablement() {
        assert!(auto_compaction_enabled(&json!({})));
        assert!(auto_compaction_enabled(&json!({
            "compaction": { "auto": true }
        })));
        assert!(!auto_compaction_enabled(&json!({
            "compaction": { "auto": false }
        })));
    }

    #[test]
    fn compaction_recent_tail_honors_turn_and_token_budget() {
        let messages = (0..6)
            .map(|index| {
                json!({
                    "type": if index % 2 == 0 { "user" } else { "assistant" },
                    "text": "message"
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            compaction_recent_count(
                &messages,
                &json!({ "compaction": { "tail_turns": 2, "preserve_recent_tokens": 2000 } })
            ),
            6
        );

        let large_messages = (0..6)
            .map(|index| {
                json!({
                    "type": if index % 2 == 0 { "user" } else { "assistant" },
                    "text": "x".repeat(9000)
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            compaction_recent_count(
                &large_messages,
                &json!({ "compaction": { "tail_turns": 2, "preserve_recent_tokens": 2000 } })
            ),
            4
        );
    }

    #[test]
    fn compaction_model_ref_rejects_unselected_models() {
        assert!(compaction_model_ref(&RunnerLlmModel::make("", "stub")).is_none());
        assert!(compaction_model_ref(&RunnerLlmModel::make("demo", "")).is_none());
        assert_eq!(
            compaction_model_ref(&RunnerLlmModel::make(" demo ", " stub ")),
            Some(ModelRef {
                id: "demo".into(),
                provider_id: "stub".into(),
                variant: None,
            })
        );
    }

    fn compaction_test_info(id: &str, model: Option<ModelRef>) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            parent_id: None,
            project_id: "prj_compaction_test".into(),
            agent: Some("build".into()),
            model,
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created: 1,
                updated: 1,
                archived: None,
            },
            title: "Compaction test".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-compaction-test".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        }
    }

    fn compaction_test_input(
        session_id: &str,
        model: RunnerLlmModel,
        message: SessionMessage,
    ) -> CompactionInput {
        CompactionInput {
            session_id: session_id.into(),
            entries: vec![HistoryEntry { seq: 1, message }],
            request: LLMRequest {
                id: None,
                model: model.clone(),
                system: Vec::new(),
                messages: Vec::new(),
                tools: Vec::new(),
                tool_choice: None,
                generation: None,
                provider_options: None,
                http: None,
            },
            model,
        }
    }

    fn large_compaction_history_entry() -> SessionMessage {
        SessionMessage::User(User {
            id: "msg_large_compaction_input".into(),
            kind: MessageKind::User,
            text: "x".repeat(120_000),
            files: None,
            agents: None,
            metadata: None,
            time: MessageTime {
                created: "1".into(),
                completed: None,
            },
        })
    }

    #[tokio::test]
    async fn automatic_compaction_uses_provider_summary() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-auto-compaction-test", None),
        );
        let session_id = "ses_auto_compaction";
        let model = ModelRef {
            id: "demo".into(),
            provider_id: "stub".into(),
            variant: None,
        };
        state.stores.write().await.sessions.insert(
            session_id.into(),
            SessionRecord {
                info: compaction_test_info(session_id, Some(model)),
                messages: vec![
                    json!({"id": "msg_old", "type": "user", "text": "old context"}),
                    json!({"id": "msg_answer", "type": "assistant", "text": "keep recent"}),
                    json!({"id": "msg_latest", "type": "user", "text": "latest"}),
                ],
                active: true,
            },
        );
        let mut events = state.events.subscribe();

        let runner_model = RunnerLlmModel::make("demo", "stub");
        let input =
            compaction_test_input(session_id, runner_model, large_compaction_history_entry());
        assert!(
            ServerCompaction {
                state: state.clone()
            }
            .compact_if_needed(input)
            .await
        );

        let compacted = events.recv().await.expect("compacted event");
        assert_eq!(compacted.r#type, "session.compacted");
        assert_eq!(compacted.data["sessionID"], session_id);
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));

        let stores = state.stores.read().await;
        let checkpoint = stores
            .sessions
            .get(session_id)
            .and_then(|record| record.messages.last())
            .expect("automatic compaction checkpoint");
        assert_eq!(checkpoint["type"], "compaction");
        assert!(checkpoint["summary"]
            .as_str()
            .expect("provider summary text")
            .starts_with("stub:"));
        assert!(checkpoint["recent"]
            .as_str()
            .expect("recent tail")
            .contains("keep recent"));
    }

    #[tokio::test]
    async fn overflow_compaction_falls_back_when_provider_summary_fails() {
        let state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-overflow-compaction-test", None),
            json!({ "compaction": { "preserve_recent_tokens": 2000 } }),
        );
        let session_id = "ses_overflow_compaction";
        let model = ModelRef {
            id: "demo".into(),
            provider_id: "unsupported-provider".into(),
            variant: None,
        };
        let messages = (0..6)
            .map(|index| {
                json!({
                    "id": format!("msg_overflow_{index}"),
                    "type": if index % 2 == 0 { "user" } else { "assistant" },
                    "text": "fallback context ".repeat(9000)
                })
            })
            .collect();
        state.stores.write().await.sessions.insert(
            session_id.into(),
            SessionRecord {
                info: compaction_test_info(session_id, Some(model)),
                messages,
                active: true,
            },
        );

        let input = compaction_test_input(
            session_id,
            RunnerLlmModel::make("demo", "unsupported-provider"),
            SessionMessage::User(User {
                id: "msg_overflow_input".into(),
                kind: MessageKind::User,
                text: "overflow".into(),
                files: None,
                agents: None,
                metadata: None,
                time: MessageTime {
                    created: "1".into(),
                    completed: None,
                },
            }),
        );
        assert!(
            ServerCompaction {
                state: state.clone()
            }
            .compact_after_overflow(input)
            .await
        );

        let stores = state.stores.read().await;
        let checkpoint = stores
            .sessions
            .get(session_id)
            .and_then(|record| record.messages.last())
            .expect("overflow compaction checkpoint");
        assert_eq!(checkpoint["type"], "compaction");
        assert!(checkpoint["summary"]
            .as_str()
            .expect("deterministic fallback summary")
            .starts_with("user: fallback context"));
    }

    #[test]
    fn server_runner_bridge_preserves_structured_provider_failure() {
        let request = oc_llm::route::transport::HttpRequestValue {
            url: "https://api.example.test/v1/chat".into(),
            body: "{}".into(),
            headers: [
                ("Authorization".into(), "Bearer request-secret".into()),
                ("x-request-id".into(), "req-123".into()),
            ]
            .into_iter()
            .collect(),
        };
        let error = oc_llm::route::executor::status_error(
            &request,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":{"message":"overloaded"},"apiKey":"body-secret"}"#,
            &std::collections::BTreeMap::from([("retry-after".into(), "2".into())]),
        );

        let error = runner_llm_error(error);
        assert_eq!(error.module, "RequestExecutor");
        assert_eq!(error.method, "execute");
        assert!(error.retryable());
        let LLMErrorReason::ProviderInternal(reason) = error.reason else {
            panic!("expected ProviderInternal reason");
        };
        assert_eq!(reason.status, 503.0);
        assert_eq!(reason.retry_after_ms, Some(2000.0));
        let http = reason.http.expect("provider HTTP context");
        assert_eq!(http.response.expect("response details").status, 503.0);
        assert_eq!(http.request.headers["Authorization"], "<redacted>");
        assert!(!http.body.expect("response body").contains("body-secret"));
    }

    #[test]
    fn mcp_resource_tools_have_strict_contracts() {
        let definitions = mcp_resource_definitions();
        assert_eq!(definitions.len(), 3);
        let read = definitions
            .iter()
            .find(|definition| definition.name == oc_session::tools::MCP_RESOURCE_TOOLS_READ)
            .expect("read resource definition");
        assert_eq!(read.input_schema["required"], json!(["server", "uri"]));
        assert_eq!(read.input_schema["additionalProperties"], false);
        assert!(is_mcp_resource_tool(
            oc_session::tools::MCP_RESOURCE_TOOLS_LIST_TEMPLATES
        ));
        assert!(!is_mcp_resource_tool("read"));
    }

    #[test]
    fn mcp_resource_target_requires_connected_server() {
        let clients = HashMap::new();
        let error = match mcp_resource_targets(&clients, Some("missing")) {
            Ok(_) => panic!("missing server should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("missing"));
        assert!(mcp_resource_targets(&clients, None).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runner_skill_guidance_surfaces_project_skill() {
        let root = std::env::temp_dir().join(format!("oc-server-skill-guidance-{}", event_id()));
        let skill = root.join(".opencode/skills/project-skill/SKILL.md");
        std::fs::create_dir_all(skill.parent().expect("skill parent")).expect("skill directory");
        std::fs::write(
            &skill,
            "---\nname: project-skill\ndescription: Use for project-specific work\n---\nProject instructions\n",
        )
        .expect("skill file");

        let location = Location::with_directory(root.to_str().expect("temporary path"), None);
        let state = AppState::new(AuthConfig::default(), CorsOptions::default(), location);
        let guidance = ServerSkillGuidance {
            state,
            session_id: "ses_skill_guidance".into(),
        };
        let context = guidance.load("build").await;

        assert!(context.baseline.contains("<available_skills>"));
        assert!(context.baseline.contains("<name>project-skill</name>"));
        assert!(context
            .baseline
            .contains("<description>Use for project-specific work</description>"));
        assert!(
            context.baseline.contains(&skill.display().to_string())
                || context
                    .baseline
                    .contains(&oc_util::fs_util::normalize_path(&skill.to_string_lossy()))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plan_exit_is_materialized_only_when_enabled() {
        let state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "experimental": { "plan_mode": true } }),
        );
        let enabled = ServerTools {
            state: state.clone(),
        }
        .materialize(&[])
        .await
        .expect("materialization");
        assert!(enabled
            .definitions
            .iter()
            .any(|definition| definition.name == oc_tool::core::plan::NAME));

        let disabled = ServerTools {
            state: AppState::new(
                AuthConfig::default(),
                CorsOptions::default(),
                Location::default_location(),
            ),
        }
        .materialize(&[])
        .await
        .expect("materialization");
        assert!(!disabled
            .definitions
            .iter()
            .any(|definition| definition.name == oc_tool::core::plan::NAME));
    }

    #[tokio::test]
    async fn configured_plugin_tool_is_materialized_and_settled() {
        let mut state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "plugin": "allow" } }),
        );
        let manager = Arc::new(oc_plugin::PluginManager::new());
        let spec = format!(
            "file://{}/tests/fixtures/example.ts",
            env!("CARGO_MANIFEST_DIR").replace("oc-server", "oc-plugin")
        );
        let report = manager.load_local(spec, json!({}), None);
        assert!(report.error.is_none(), "plugin failed to load: {report:?}");
        state
            .plugin_reports
            .lock()
            .expect("plugin report lock poisoned")
            .push(report);
        state.plugin_manager = Some(manager);

        let materialization = ServerTools { state }.materialize(&[]).await.expect("tools");
        assert!(materialization
            .definitions
            .iter()
            .any(|definition| definition.name == "mytool"));
        let settlement = materialization
            .settle
            .settle(oc_session_runner::session::services::ExecuteInput {
                session_id: "ses_plugin_tool".into(),
                agent: "build".into(),
                assistant_message_id: "msg_plugin_tool".into(),
                call: oc_session_runner::session::services::ToolCall {
                    id: "call_plugin_tool".into(),
                    name: "mytool".into(),
                    input: json!({ "foo": "world" }),
                    provider_executed: false,
                    provider_metadata: None,
                },
            })
            .await
            .expect("plugin tool settlement");
        assert_eq!(
            settlement.result,
            ToolResultValue::Json {
                value: json!("Hello world!")
            }
        );
    }

    #[tokio::test]
    async fn running_async_plugin_tool_is_interrupted_by_session_abort() {
        struct StartHost {
            started: Arc<std::sync::atomic::AtomicBool>,
        }

        impl oc_plugin::PluginHost for StartHost {
            fn log(&self, _level: &str, message: &str) {
                if message == "server-cancellation-started" {
                    self.started
                        .store(true, std::sync::atomic::Ordering::Release);
                }
            }
        }

        let path = std::env::temp_dir().join(format!(
            "oc-server-plugin-cancellation-{}.ts",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"
export default {
  server: async () => ({
    tool: {
      cancellable: {
        description: "cancellable",
        args: {},
        execute: async (_args, context) => {
          console.log("server-cancellation-started")
          let notified = false
          context.abort.addEventListener("abort", () => { notified = true })
          while (!notified) {
            await Promise.resolve()
          }
          return { observed: context.abort.aborted, notified }
        },
      },
    },
  }),
}
"#,
        )
        .unwrap();

        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let manager = Arc::new(oc_plugin::PluginManager::with_host(Arc::new(StartHost {
            started: Arc::clone(&started),
        })));
        let report = manager.load_local(format!("file://{}", path.display()), json!({}), None);
        assert!(
            report.error.is_none(),
            "cancellation fixture failed: {report:?}"
        );

        let mut state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "plugin": "allow" } }),
        );
        state
            .plugin_reports
            .lock()
            .expect("plugin report lock poisoned")
            .push(report);
        state.plugin_manager = Some(manager);
        let token = state
            .acquire_session_run("ses_plugin_cancel")
            .await
            .expect("session run token");
        let materialization = ServerTools {
            state: state.clone(),
        }
        .materialize(&[])
        .await
        .expect("plugin tools");
        let settle = materialization.settle;
        let token_for_outer = token.clone();
        let task = tokio::spawn(async move {
            let execute = settle.settle(oc_session_runner::session::services::ExecuteInput {
                session_id: "ses_plugin_cancel".into(),
                agent: "build".into(),
                assistant_message_id: "msg_plugin_cancel".into(),
                call: oc_session_runner::session::services::ToolCall {
                    id: "call_plugin_cancel".into(),
                    name: "cancellable".into(),
                    input: json!({}),
                    provider_executed: false,
                    provider_metadata: None,
                },
            });
            tokio::select! {
                _ = token_for_outer.cancelled() => Err(ToolSettlementError::Interrupted),
                result = execute => result,
            }
        });

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while !started.load(std::sync::atomic::Ordering::Acquire) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "async tool did not start"
            );
            tokio::task::yield_now().await;
        }
        token.cancel();

        let result = task.await.expect("server settlement task panicked");
        assert!(matches!(result, Err(ToolSettlementError::Interrupted)));
        let manager = state
            .plugin_manager
            .as_ref()
            .expect("plugin manager")
            .clone();
        tokio::task::spawn_blocking(move || manager.dispose())
            .await
            .expect("plugin disposal task panicked")
            .expect("plugin owner thread did not drain after cancellation");
        let _ = state.finish_session_run("ses_plugin_cancel").await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn configured_permission_rules_override_interactive_defaults() {
        let denied = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "bash": { "rm *": "deny" } } }),
        );
        assert!(
            !permission_gate(
                &denied,
                "ses_permission_policy",
                "bash",
                "rm -rf build",
                true,
                json!({ "tool": "bash" }),
            )
            .await
        );

        let allowed = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "bash": "allow" } }),
        );
        assert!(
            permission_gate(
                &allowed,
                "ses_permission_policy",
                "bash",
                "echo safe",
                true,
                json!({ "tool": "bash" }),
            )
            .await
        );

        let default_read = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        assert!(
            permission_gate(
                &default_read,
                "ses_permission_policy",
                "read",
                "README.md",
                false,
                json!({ "tool": "read" }),
            )
            .await
        );

        let mcp_allowed = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "mcp": { "server:tool": "allow" } } }),
        );
        assert!(
            permission_gate(
                &mcp_allowed,
                "ses_permission_policy",
                "mcp",
                "server:tool",
                true,
                json!({ "tool": "mcp_server_tool" }),
            )
            .await
        );

        let mcp_denied = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "mcp": { "server:*": "deny" } } }),
        );
        assert!(
            !permission_gate(
                &mcp_denied,
                "ses_permission_policy",
                "mcp",
                "server:tool",
                true,
                json!({ "tool": "mcp_server_tool" }),
            )
            .await
        );
    }

    #[tokio::test]
    async fn authorize_tool_maps_lsp_to_lsp_permission() {
        let allowed = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "lsp": "allow" } }),
        );
        assert!(
            authorize_tool(
                &allowed,
                "ses_authorize_lsp",
                &allowed.location.directory,
                "lsp",
                &json!({ "filePath": "Cargo.toml", "operation": "documentSymbol" }),
            )
            .await
        );

        let denied = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "permission": { "lsp": "deny" } }),
        );
        assert!(
            !authorize_tool(
                &denied,
                "ses_authorize_lsp",
                &denied.location.directory,
                "lsp",
                &json!({ "filePath": "Cargo.toml", "operation": "documentSymbol" }),
            )
            .await
        );

        // Unknown tool families are declined without a permission request.
        let plain = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        assert!(
            !authorize_tool(
                &plain,
                "ses_authorize_lsp",
                &plain.location.directory,
                "bogus_tool",
                &json!({}),
            )
            .await
        );
    }

    #[tokio::test]
    async fn plan_agent_history_gets_plan_mode_reminder() {
        let state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            json!({ "experimental": { "plan_mode": true } }),
        );
        let directory =
            std::env::temp_dir().join(format!("opencode-plan-reminder-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let session_id = "ses_plan_reminder".to_string();
        state.stores.write().await.sessions.insert(
            session_id.clone(),
            SessionRecord {
                info: crate::schema::SessionInfo {
                    id: session_id.clone(),
                    parent_id: None,
                    project_id: "prj_plan".into(),
                    agent: Some("plan".into()),
                    model: None,
                    cost: 0.0,
                    tokens: crate::schema::Tokens {
                        input: 0.0,
                        output: 0.0,
                        reasoning: 0.0,
                        cache: crate::schema::CacheTokens {
                            read: 0.0,
                            write: 0.0,
                        },
                    },
                    time: crate::schema::SessionTime {
                        created: 7,
                        updated: 7,
                        archived: None,
                    },
                    title: "Plan".into(),
                    location: crate::schema::LocationRef {
                        directory: directory.to_string_lossy().into_owned(),
                        workspace_id: None,
                    },
                    subpath: None,
                    revert: None,
                },
                messages: Vec::new(),
                active: false,
            },
        );
        let mut entries = vec![HistoryEntry {
            seq: 1,
            message: SessionMessage::User(oc_session_runner::session::message::User {
                id: "msg_user".into(),
                kind: MessageKind::User,
                text: "plan a feature".into(),
                files: None,
                agents: None,
                metadata: None,
                time: oc_session_runner::session::message::MessageTime {
                    created: 1.to_string(),
                    completed: None,
                },
            }),
        }];
        apply_plan_reminders(&state, &session_id, &mut entries).await;

        let SessionMessage::User(user) = &entries[0].message else {
            panic!("expected user message");
        };
        assert!(
            user.text.contains("Plan mode is active"),
            "user text: {}",
            user.text
        );
        assert!(
            user.text.contains("No plan file exists yet"),
            "plan-file lifecycle reminder missing: {}",
            user.text
        );
        // Non-VCS sessions keep plans under the global data directory; the
        // `ensureDir` lifecycle step created it for the fresh plan agent.
        let data_dir = oc_mcp::auth::default_data_dir();
        assert!(
            data_dir.join("plans").is_dir(),
            "expected {} to be created",
            data_dir.join("plans").display()
        );
        let _ = std::fs::remove_dir_all(data_dir.join("plans"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[tokio::test]
    async fn approved_plan_exit_switches_session_to_build() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let session_id = "ses_plan_exit".to_string();
        state.stores.write().await.sessions.insert(
            session_id.clone(),
            SessionRecord {
                info: crate::schema::SessionInfo {
                    id: session_id.clone(),
                    parent_id: None,
                    project_id: "prj_plan_exit".into(),
                    agent: Some("plan".into()),
                    model: None,
                    cost: 0.0,
                    tokens: crate::schema::Tokens {
                        input: 0.0,
                        output: 0.0,
                        reasoning: 0.0,
                        cache: crate::schema::CacheTokens {
                            read: 0.0,
                            write: 0.0,
                        },
                    },
                    time: crate::schema::SessionTime {
                        created: 1,
                        updated: 1,
                        archived: None,
                    },
                    title: "Plan exit".into(),
                    location: crate::schema::LocationRef {
                        directory: "/tmp".into(),
                        workspace_id: None,
                    },
                    subpath: None,
                    revert: None,
                },
                messages: Vec::new(),
                active: false,
            },
        );
        let task = tokio::spawn(settle_plan_exit(
            state.clone(),
            oc_session_runner::session::services::ExecuteInput {
                session_id: session_id.clone(),
                agent: "plan".into(),
                assistant_message_id: "msg_plan_exit".into(),
                call: oc_session_runner::session::services::ToolCall {
                    id: "call_plan_exit".into(),
                    name: oc_tool::core::plan::NAME.into(),
                    input: json!({}),
                    provider_executed: false,
                    provider_metadata: None,
                },
            },
        ));
        let request = loop {
            if let Some(request) = state.question_service.list().into_iter().next() {
                break request;
            }
            tokio::task::yield_now().await;
        };
        state
            .question_service
            .reply(&request.id, vec![vec!["Yes".into()]])
            .expect("reply to plan question");
        task.await
            .expect("plan task")
            .expect("approved plan settlement");
        assert_eq!(
            state
                .stores
                .read()
                .await
                .sessions
                .get(&session_id)
                .and_then(|record| record.info.agent.as_deref()),
            Some("build")
        );
    }

    #[tokio::test]
    async fn runner_events_project_back_to_server_messages() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-runner-test", None),
        );
        let session = crate::schema::SessionInfo {
            id: "ses_runner_projection".into(),
            parent_id: None,
            project_id: "prj_runner_projection".into(),
            agent: Some("build".into()),
            model: Some(crate::schema::ModelRef {
                id: "test-model".into(),
                provider_id: "openai".into(),
                variant: None,
            }),
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created: 1,
                updated: 1,
                archived: None,
            },
            title: "Runner projection".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-runner-test".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        };
        state.stores.write().await.sessions.insert(
            session.id.clone(),
            SessionRecord {
                info: session.clone(),
                messages: Vec::new(),
                active: true,
            },
        );
        let mut receiver = state.events.subscribe();
        let bus = ServerEventBus {
            state: state.clone(),
            turns: Arc::new(Mutex::new(HashMap::new())),
        };
        let timestamp = timestamp_now();
        bus.publish(SessionEvent::StepStarted {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            agent: "build".into(),
            model: RunnerModelRef {
                id: "test-model".into(),
                provider_id: "openai".into(),
                variant: None,
            },
            snapshot: None,
        })
        .await;
        bus.publish(SessionEvent::ReasoningStarted {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            reasoning_id: "reasoning_1".into(),
            provider_metadata: None,
        })
        .await;
        bus.publish(SessionEvent::ReasoningDelta {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            reasoning_id: "reasoning_1".into(),
            delta: "plan".into(),
        })
        .await;
        bus.publish(SessionEvent::ReasoningEnded {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            reasoning_id: "reasoning_1".into(),
            text: "plan".into(),
            provider_metadata: None,
        })
        .await;
        bus.publish(SessionEvent::TextStarted {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            text_id: "text_1".into(),
        })
        .await;
        bus.publish(SessionEvent::TextDelta {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            text_id: "text_1".into(),
            delta: "hello".into(),
        })
        .await;
        bus.publish(SessionEvent::TextEnded {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            text_id: "text_1".into(),
            text: "hello".into(),
        })
        .await;
        bus.publish(SessionEvent::ToolInputStarted {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            call_id: "call_read".into(),
            name: "read".into(),
        })
        .await;
        bus.publish(SessionEvent::ToolInputDelta {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            call_id: "call_read".into(),
            delta: r#"{"path":"README.md"}"#.into(),
        })
        .await;
        bus.publish(SessionEvent::ToolInputEnded {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            call_id: "call_read".into(),
            text: r#"{"path":"README.md"}"#.into(),
        })
        .await;
        bus.publish(SessionEvent::ToolCalled {
            timestamp: timestamp_now(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            call_id: "call_read".into(),
            tool: "read".into(),
            input: [("path".to_string(), Value::String("README.md".into()))]
                .into_iter()
                .collect(),
            provider: Provider::new(false, None),
        })
        .await;
        bus.publish(SessionEvent::ToolProgress {
            timestamp: timestamp.clone(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            call_id: "call_read".into(),
            structured: [("bytes".to_string(), Value::Number(2.into()))]
                .into_iter()
                .collect(),
            content: vec![ToolContent::text("he")],
        })
        .await;
        bus.publish(SessionEvent::ToolSuccess {
            timestamp: timestamp_now(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            call_id: "call_read".into(),
            structured: [("bytes".to_string(), Value::Number(5.into()))]
                .into_iter()
                .collect(),
            content: vec![ToolContent::text("hello")],
            output_paths: None,
            result: Some(ToolResultValue::Text {
                value: Value::String("hello".into()),
            }),
            provider: Provider::new(false, None),
        })
        .await;
        bus.publish(SessionEvent::StepEnded {
            timestamp,
            session_id: session.id.clone(),
            assistant_message_id: "msg_assistant".into(),
            finish: "stop".into(),
            cost: 1.25,
            tokens: Tokens {
                input: 1.0,
                output: 2.0,
                reasoning: 0.0,
                cache: CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            snapshot: None,
            files: None,
        })
        .await;

        let stores = state.stores.read().await;
        let record = stores
            .sessions
            .get(&session.id)
            .expect("session projection");
        assert!(!record.active);
        assert_eq!(record.info.cost, 1.25);
        assert_eq!(record.info.tokens.input, 1.0);
        assert_eq!(record.info.tokens.output, 2.0);
        assert_eq!(record.messages[0]["cost"], 1.25);
        assert_eq!(record.messages[0]["text"], "hello");
        assert_eq!(record.messages[0]["reasoning"], "plan");
        assert_eq!(record.messages[0]["content"][0]["type"], "reasoning");
        assert_eq!(record.messages[0]["content"][1]["type"], "text");
        assert_eq!(record.messages[0]["content"][2]["type"], "tool");
        assert_eq!(
            record.messages[0]["content"][2]["state"]["status"],
            "completed"
        );
        assert_eq!(record.messages[0]["content"][2]["id"], "call_read");
        assert_eq!(
            record.messages[0]["content"][2]["state"]["input"]["path"],
            "README.md"
        );
        let lowered = server_message(&record.messages[0]).expect("assistant lowers");
        let SessionMessage::Assistant(assistant) = lowered else {
            panic!("expected assistant message")
        };
        assert!(assistant
            .content
            .iter()
            .any(|content| content.as_tool().is_some()));
        drop(stores);

        bus.publish(SessionEvent::StepStarted {
            timestamp: timestamp_now(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_failed".into(),
            agent: "build".into(),
            model: RunnerModelRef {
                id: "test-model".into(),
                provider_id: "openai".into(),
                variant: None,
            },
            snapshot: None,
        })
        .await;
        bus.publish(SessionEvent::Retried {
            timestamp: timestamp_now(),
            session_id: session.id.clone(),
            attempt: 2.0,
            error: RetryError {
                message: "temporary provider overload".into(),
                status_code: Some(503.0),
                is_retryable: true,
                response_headers: None,
                response_body: None,
                metadata: None,
            },
        })
        .await;
        bus.publish(SessionEvent::StepFailed {
            timestamp: timestamp_now(),
            session_id: session.id.clone(),
            assistant_message_id: "msg_failed".into(),
            error: oc_session_runner::session::message::UnknownError::new("provider failed"),
        })
        .await;

        let stores = state.stores.read().await;
        let record = stores.sessions.get(&session.id).expect("failed projection");
        assert!(!record.active);
        assert_eq!(record.messages.len(), 2);
        assert_eq!(record.messages[1]["finish"], "error");
        assert_eq!(record.messages[1]["error"]["message"], "provider failed");
        drop(stores);

        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        assert!(events
            .iter()
            .any(|event| event.r#type == "message.part.updated"));
        assert!(events.iter().any(|event| {
            event.r#type == "message.part.updated"
                && event.data["part"]["id"] == "call_read"
                && event.data["part"]["state"]["status"] == "pending"
                && event.data["part"]["state"]["input"] == r#"{"path":"README.md"}"#
        }));
        assert!(events.iter().any(|event| {
            event.r#type == "message.part.updated"
                && event.data["part"]["id"] == "call_read"
                && event.data["part"]["state"]["status"] == "running"
                && event.data["part"]["state"]["structured"]["bytes"] == 2
        }));
        assert!(events.iter().any(|event| {
            event.r#type == "message.part.updated"
                && event.data["part"]["type"] == "retry"
                && event.data["part"]["attempt"] == 2.0
                && event.data["part"]["error"]["message"] == "temporary provider overload"
        }));
        assert!(events.iter().any(|event| event.r#type == "message.updated"));
        assert!(events.iter().any(|event| {
            event.r#type == "session.usage.updated"
                && event.data["sessionID"] == session.id
                && event.data["cost"] == 1.25
                && event.data["tokens"]["input"] == 1.0
                && event.data["tokens"]["output"] == 2.0
        }));
        assert!(events.iter().any(|event| {
            event.r#type == "session.status"
                && event.data["status"]["type"] == "retry"
                && event.data["status"]["attempt"] == 2.0
        }));
        assert!(events.iter().any(|event| {
            event.r#type == "session.retry.scheduled"
                && event.data["sessionID"] == session.id
                && event.data["assistantMessageID"] == "msg_failed"
                && event.data["attempt"] == 2.0
                && event.data["error"]["message"] == "temporary provider overload"
        }));
        assert!(events.iter().any(|event| {
            event.r#type == "session.error"
                && event.data["error"]["data"]["message"] == "provider failed"
        }));
    }

    #[tokio::test]
    async fn server_history_applies_compaction_and_revert_boundaries() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-history-test", None),
        );
        let session = crate::schema::SessionInfo {
            id: "ses_history_boundaries".into(),
            parent_id: None,
            project_id: "prj_history_boundaries".into(),
            agent: Some("build".into()),
            model: None,
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created: 1,
                updated: 1,
                archived: None,
            },
            title: "History boundaries".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-history-test".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: Some(json!({ "messageID": "msg_after" })),
        };
        state.stores.write().await.sessions.insert(
            session.id.clone(),
            SessionRecord {
                info: session,
                messages: vec![
                    json!({ "id": "msg_before", "type": "user", "text": "old", "time": { "created": 1 } }),
                    json!({ "id": "msg_checkpoint", "type": "compaction", "reason": "manual", "summary": "old summary", "recent": "[]", "time": { "created": 2 } }),
                    json!({ "id": "msg_recent", "type": "user", "text": "recent", "time": { "created": 3 } }),
                    json!({ "id": "msg_after", "type": "user", "text": "rolled back", "time": { "created": 4 } }),
                ],
                active: false,
            },
        );
        let history = ServerHistory { state };
        let entries = history
            .entries_for_runner(&"ses_history_boundaries".into(), 0)
            .await;
        let ids = entries
            .iter()
            .map(|entry| match &entry.message {
                SessionMessage::Compaction(message) => message.id.as_str(),
                SessionMessage::User(message) => message.id.as_str(),
                _ => "unexpected",
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["msg_checkpoint", "msg_recent"]);
    }

    #[tokio::test]
    async fn server_history_holds_unpromoted_input_out_of_provider_context() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-pending-input-test", None),
        );
        let session_id = "ses_pending_input";
        state.stores.write().await.sessions.insert(
            session_id.into(),
            SessionRecord {
                info: compaction_test_info(session_id, None),
                messages: vec![
                    json!({ "id": "msg_promoted", "type": "user", "text": "first", "time": { "created": 1 } }),
                    json!({ "id": "msg_pending", "type": "user", "text": "later", "time": { "created": 2 } }),
                ],
                active: true,
            },
        );
        state
            .enqueue_session_input(session_id, "msg_pending", json!({}), 2, "queue")
            .await;

        let entries = (ServerHistory { state })
            .entries_for_runner(&session_id.into(), 0)
            .await;
        assert_eq!(entries.len(), 1);
        let SessionMessage::User(message) = &entries[0].message else {
            panic!("expected user message")
        };
        assert_eq!(message.id, "msg_promoted");
    }

    #[tokio::test]
    async fn durable_history_prefers_sqlite_session_messages() {
        let database = Arc::new(oc_database::Database::open_memory().expect("database"));
        let state = AppState::with_database(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-durable-history-test", None),
            database.clone(),
        )
        .expect("state");
        let session = crate::schema::SessionInfo {
            id: "ses_durable_history".into(),
            parent_id: None,
            project_id: "prj_durable_history".into(),
            agent: Some("build".into()),
            model: None,
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created: 1,
                updated: 1,
                archived: None,
            },
            title: "Durable history".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-durable-history-test".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        };
        state.persist_session(&session);
        database
            .insert(
                "session_message",
                &oc_database::tables::SessionMessageRow {
                    id: "msg_durable".into(),
                    session_id: session.id.clone(),
                    r#type: "user".into(),
                    seq: 1,
                    time_created: 1,
                    time_updated: 1,
                    data: json!({
                        "role": "user",
                        "text": "from sqlite",
                        "time": { "created": 1 }
                    }),
                },
                oc_database::tables::json_columns("session_message"),
            )
            .expect("session message");

        let history = ServerHistory { state };
        let entries = history
            .entries_for_runner(&"ses_durable_history".into(), 0)
            .await;
        assert_eq!(entries.len(), 1);
        let SessionMessage::User(message) = &entries[0].message else {
            panic!("expected durable user message")
        };
        assert_eq!(message.id, "msg_durable");
        assert_eq!(message.text, "from sqlite");
    }

    #[tokio::test]
    async fn read_only_core_tools_settle_inside_workspace() {
        let root = std::env::temp_dir().join(format!("oc-server-tool-{}", event_id()));
        std::fs::create_dir_all(&root).expect("tool test directory");
        std::fs::write(root.join("hello.txt"), "hello from tool").expect("tool test file");
        let location = root.to_string_lossy().to_string();
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory(&location, None),
        );
        let materialization = ServerTools { state }.materialize(&[]).await.expect("tools");
        assert!(materialization
            .definitions
            .iter()
            .any(|definition| definition.name == "read"));
        let settlement = materialization
            .settle
            .settle(oc_session_runner::session::services::ExecuteInput {
                session_id: "ses_tool".into(),
                agent: "build".into(),
                assistant_message_id: "msg_tool".into(),
                call: oc_session_runner::session::services::ToolCall {
                    id: "call_read".into(),
                    name: "read".into(),
                    input: json!({ "path": "hello.txt" }),
                    provider_executed: false,
                    provider_metadata: None,
                },
            })
            .await
            .expect("read settlement");
        let value = match settlement.result {
            oc_session_runner::llm::ToolResultValue::Json { value }
            | oc_session_runner::llm::ToolResultValue::Text { value } => value,
            other => panic!("unexpected read result: {other:?}"),
        };
        assert!(value.to_string().contains("hello from tool"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn apply_patch_core_tool_settles_through_server_runner() {
        let root = std::env::temp_dir().join(format!("oc-server-apply-patch-{}", event_id()));
        std::fs::create_dir_all(&root).expect("tool test directory");
        let location = root.to_string_lossy().to_string();
        let state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory(&location, None),
            json!({ "permission": { "edit": "allow" } }),
        );
        let materialization = ServerTools { state }.materialize(&[]).await.expect("tools");
        let settlement = materialization
            .settle
            .settle(oc_session_runner::session::services::ExecuteInput {
                session_id: "ses_patch".into(),
                agent: "build".into(),
                assistant_message_id: "msg_patch".into(),
                call: oc_session_runner::session::services::ToolCall {
                    id: "call_patch".into(),
                    name: "apply_patch".into(),
                    input: json!({
                        "patchText": "*** Begin Patch\n*** Add File: created.txt\n+created by apply_patch\n*** End Patch"
                    }),
                    provider_executed: false,
                    provider_metadata: None,
                },
            })
            .await
            .expect("apply patch settlement");
        assert!(root.join("created.txt").is_file());
        assert!(std::fs::read_to_string(root.join("created.txt"))
            .expect("created file")
            .contains("created by apply_patch"));
        let value = match settlement.result {
            oc_session_runner::llm::ToolResultValue::Json { value }
            | oc_session_runner::llm::ToolResultValue::Text { value } => value,
            other => panic!("unexpected apply_patch result: {other:?}"),
        };
        assert!(
            value.to_string().contains("A created.txt"),
            "value={value:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stub_provider_runs_a_real_session_turn() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-stub-runner-test", None),
        );
        let session_id = "ses_stub_runner";
        let created = now_millis();
        let session = crate::schema::SessionInfo {
            id: session_id.into(),
            parent_id: None,
            project_id: "prj_stub_runner".into(),
            agent: Some("build".into()),
            model: Some(crate::schema::ModelRef {
                id: "demo".into(),
                provider_id: "stub".into(),
                variant: None,
            }),
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created,
                updated: created,
                archived: None,
            },
            title: "Stub runner".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-stub-runner-test".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        };
        state.stores.write().await.sessions.insert(
            session_id.into(),
            SessionRecord {
                info: session,
                messages: vec![json!({
                    "id": "msg_stub_user",
                    "type": "user",
                    "text": "hello",
                    "time": {"created": created}
                })],
                active: true,
            },
        );
        let mut events = state.events.subscribe();

        run_session(state.clone(), session_id.into()).await;

        let stores = state.stores.read().await;
        let record = stores.sessions.get(session_id).expect("stub session");
        let mut captured = Vec::new();
        while let Ok(event) = events.try_recv() {
            captured.push((event.r#type, event.data));
        }
        assert_eq!(
            record.messages.len(),
            2,
            "runner should append an assistant; events={captured:?}"
        );
        assert_eq!(record.messages[1]["type"], "assistant");
        assert_eq!(record.messages[1]["text"], "stub: hello");
        assert!(!record.active);
    }

    #[tokio::test]
    async fn foreground_subagent_creates_parented_child_and_returns_output() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-subagent-runner-test", None),
        );
        let parent_id = "ses_subagent_parent";
        let created = now_millis();
        let parent = crate::schema::SessionInfo {
            id: parent_id.into(),
            parent_id: None,
            project_id: "prj_subagent_runner".into(),
            agent: Some("build".into()),
            model: Some(crate::schema::ModelRef {
                id: "demo".into(),
                provider_id: "stub".into(),
                variant: None,
            }),
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created,
                updated: created,
                archived: None,
            },
            title: "Subagent parent".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-subagent-runner-test".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        };
        state.stores.write().await.sessions.insert(
            parent_id.into(),
            SessionRecord {
                info: parent,
                messages: vec![],
                active: true,
            },
        );

        let result = execute_subagent(
            state.clone(),
            oc_tool::model::SubagentRequest {
                parent_session_id: parent_id.into(),
                parent_message_id: "msg_parent".into(),
                description: "Explore the repository".into(),
                prompt: "inspect the project".into(),
                subagent_type: "explore".into(),
                task_id: Some("ses_subagent_child".into()),
                command: None,
                background: false,
            },
        )
        .await
        .expect("foreground subagent");

        assert_eq!(result.session_id, "ses_subagent_child");
        assert_eq!(result.state, "completed");
        assert_eq!(result.output, "stub: inspect the project");

        let stores = state.stores.read().await;
        let child = stores
            .sessions
            .get("ses_subagent_child")
            .expect("child session");
        assert_eq!(child.info.parent_id.as_deref(), Some(parent_id));
        assert_eq!(child.info.agent.as_deref(), Some("explore"));
        assert_eq!(child.info.title, "Explore the repository");
        assert_eq!(child.messages[0]["type"], "user");
        assert_eq!(child.messages[0]["text"], "inspect the project");
        assert_eq!(child.messages[1]["type"], "assistant");
        assert_eq!(child.messages[1]["text"], "stub: inspect the project");
        assert!(!child.active);

        drop(stores);
        let resumed = execute_subagent(
            state.clone(),
            oc_tool::model::SubagentRequest {
                parent_session_id: parent_id.into(),
                parent_message_id: "msg_parent_2".into(),
                description: "Resume the repository task".into(),
                prompt: "continue the project inspection".into(),
                subagent_type: "explore".into(),
                task_id: Some("ses_subagent_child".into()),
                command: None,
                background: false,
            },
        )
        .await
        .expect("resumed foreground subagent");
        assert_eq!(resumed.state, "completed");
        assert_eq!(resumed.output, "stub: continue the project inspection");
        let stores = state.stores.read().await;
        let child = stores
            .sessions
            .get("ses_subagent_child")
            .expect("resumed child session");
        assert_eq!(child.messages.len(), 4);
        assert_eq!(child.messages[2]["type"], "user");
        assert_eq!(child.messages[3]["type"], "assistant");
        assert!(!child.active);
    }

    #[tokio::test]
    async fn aborted_parent_does_not_admit_foreground_child() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-aborted-subagent-test", None),
        );
        let parent_id = "ses_aborted_subagent_parent";
        let mut parent = compaction_test_info(
            parent_id,
            Some(ModelRef {
                id: "demo".into(),
                provider_id: "stub".into(),
                variant: None,
            }),
        );
        parent.title = "Aborted parent".into();
        state.stores.write().await.sessions.insert(
            parent_id.into(),
            SessionRecord {
                info: parent,
                messages: Vec::new(),
                active: false,
            },
        );
        let parent_token = state
            .acquire_session_run(parent_id)
            .await
            .expect("parent run token");
        parent_token.cancel();

        let error = execute_subagent(
            state.clone(),
            oc_tool::model::SubagentRequest {
                parent_session_id: parent_id.into(),
                parent_message_id: "msg_aborted_parent".into(),
                description: "Should not start".into(),
                prompt: "this must not run".into(),
                subagent_type: "explore".into(),
                task_id: Some("ses_aborted_subagent_child".into()),
                command: None,
                background: false,
            },
        )
        .await
        .expect_err("aborted parent must cancel before child admission");

        assert!(error.contains("parent session `ses_aborted_subagent_parent` was aborted"));
        assert!(!state
            .stores
            .read()
            .await
            .sessions
            .contains_key("ses_aborted_subagent_child"));
    }

    #[tokio::test]
    async fn background_subagent_returns_running_and_completes_durably() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-background-subagent-test", None),
        );
        let parent_id = "ses_background_parent";
        let created = now_millis();
        let parent = crate::schema::SessionInfo {
            id: parent_id.into(),
            parent_id: None,
            project_id: "prj_background_runner".into(),
            agent: Some("build".into()),
            model: Some(crate::schema::ModelRef {
                id: "demo".into(),
                provider_id: "stub".into(),
                variant: None,
            }),
            cost: 0.0,
            tokens: crate::schema::Tokens {
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::schema::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            time: crate::schema::SessionTime {
                created,
                updated: created,
                archived: None,
            },
            title: "Background parent".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-background-subagent-test".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        };
        state.stores.write().await.sessions.insert(
            parent_id.into(),
            SessionRecord {
                info: parent,
                messages: vec![],
                active: true,
            },
        );

        let result = execute_subagent(
            state.clone(),
            oc_tool::model::SubagentRequest {
                parent_session_id: parent_id.into(),
                parent_message_id: "msg_background".into(),
                description: "Run in background".into(),
                prompt: "background inspection".into(),
                subagent_type: "explore".into(),
                task_id: Some("ses_background_child".into()),
                command: None,
                background: true,
            },
        )
        .await
        .expect("background subagent");
        assert_eq!(result.state, "running");
        assert!(result.output.is_empty());

        for _ in 0..50 {
            let finished = state
                .stores
                .read()
                .await
                .sessions
                .get("ses_background_child")
                .is_some_and(|child| {
                    !child.active
                        && child
                            .messages
                            .iter()
                            .any(|message| message["type"] == "assistant")
                });
            if finished {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let stores = state.stores.read().await;
        let child = stores
            .sessions
            .get("ses_background_child")
            .expect("background child session");
        assert!(!child.active);
        assert_eq!(child.info.parent_id.as_deref(), Some(parent_id));
        assert_eq!(child.messages[1]["text"], "stub: background inspection");
    }
}
