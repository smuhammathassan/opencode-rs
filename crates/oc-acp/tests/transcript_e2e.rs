#![allow(clippy::all)]
//! In-process end-to-end transcript test for the ACP service. Drives a full
//! JSON-RPC session lifecycle against a scripted fake provider and asserts the
//! exact sequence of `session/update` notifications produced for streaming
//! text + tool parts, session idle, and cancellation.
//!
//! Transcript: initialize → session/new → prompt → streaming session/update
//! (text + tool) → session idle → cancel (stopReason: cancelled).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures::stream::BoxStream;
use futures::StreamExt;
use oc_acp::connection::AgentSideConnection;
use oc_acp::sdk::{
    AgentInfo, Config, Event, ModelInfo, ModelLimit, OpencodeClient, Part, ProviderInfo, SdkError,
    Session, SessionMessageResponse, SessionStatus, SessionStatusProperties, SessionTime, ToolPart,
    ToolState, ToolStateCompleted,
};
use oc_acp::service::{Service, ServiceInput};
use oc_acp::types::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, SessionUpdate, StopReason,
    TextContent,
};
use serde_json::{json, Map, Value};

/// A scripted fake opencode SDK whose global event stream replays a queued
/// script of events (parts, idle) so the ACP subscription emits deterministic
/// `session/update` notifications.
struct ScriptedSdk {
    providers: Vec<ProviderInfo>,
    agents: Vec<AgentInfo>,
    config: Config,
    event_script: Arc<Mutex<VecDeque<Event>>>,
    script_consumed: Arc<Mutex<bool>>,
    prompt_error: Mutex<Option<Value>>,
    aborted: Mutex<Vec<String>>,
    assistant_message: Mutex<Option<SessionMessageResponse>>,
}

impl Default for ScriptedSdk {
    fn default() -> Self {
        let model = ModelInfo {
            id: "claude-sonnet-4".into(),
            provider_id: "anthropic".into(),
            name: "Claude Sonnet 4".into(),
            variants: None,
            limit: Some(ModelLimit {
                context: 200_000.0,
                output: 64_000.0,
            }),
        };
        let mut models = indexmap::IndexMap::new();
        models.insert("claude-sonnet-4".to_string(), model);
        let provider = ProviderInfo {
            id: "anthropic".into(),
            name: "anthropic".into(),
            source: "env".into(),
            env: vec![],
            key: None,
            options: Map::new(),
            models,
        };
        Self {
            providers: vec![provider],
            agents: vec![AgentInfo {
                name: "build".into(),
                description: Some("The default mode".into()),
                mode: "primary".into(),
                hidden: None,
            }],
            config: Config {
                model: Some("anthropic/claude-sonnet-4".into()),
            },
            event_script: Arc::new(Mutex::new(VecDeque::new())),
            script_consumed: Arc::new(Mutex::new(false)),
            prompt_error: Mutex::new(None),
            aborted: Mutex::new(Vec::new()),
            assistant_message: Mutex::new(None),
        }
    }
}

impl ScriptedSdk {
    fn queue(&self, script: Vec<Event>) {
        let mut queue = self.event_script.lock().unwrap();
        queue.extend(script);
        *self.script_consumed.lock().unwrap() = false;
    }
}

#[async_trait::async_trait]
impl OpencodeClient for ScriptedSdk {
    fn global_event(&self) -> BoxStream<'static, Option<Event>> {
        let script = Arc::clone(&self.event_script);
        let consumed = Arc::clone(&self.script_consumed);
        futures::stream::unfold((script, consumed), |(script, consumed)| async move {
            let event = {
                let mut queue = script.lock().unwrap();
                let event = queue.pop_front();
                if event.is_none() {
                    *consumed.lock().unwrap() = true;
                }
                event
            };
            event.map(|event| (Some(event), (script, consumed)))
        })
        .boxed()
    }

    async fn session_create(
        &self,
        _request: oc_acp::sdk::SessionCreateRequest,
    ) -> Result<Session, SdkError> {
        Ok(Session {
            id: "s1".into(),
            directory: "/tmp".into(),
            title: "New session".into(),
            time: SessionTime {
                created: 1,
                updated: 1,
            },
        })
    }

    async fn session_get(&self, _directory: &str, _session_id: &str) -> Result<Session, SdkError> {
        Ok(Session {
            id: "s1".into(),
            directory: "/tmp".into(),
            title: "Existing".into(),
            time: SessionTime {
                created: 1,
                updated: 2,
            },
        })
    }

    async fn session_messages(
        &self,
        _directory: &str,
        _session_id: &str,
        _limit: Option<u32>,
    ) -> Result<Vec<SessionMessageResponse>, SdkError> {
        Ok(Vec::new())
    }

    async fn session_message(
        &self,
        _directory: &str,
        _session_id: &str,
        message_id: &str,
    ) -> Result<SessionMessageResponse, SdkError> {
        let stored = self.assistant_message.lock().unwrap().clone();
        if let Some(message) = stored {
            if message.info.id() == message_id {
                return Ok(message);
            }
        }
        Err(Value::Null)
    }

    async fn session_list(&self, _directory: Option<&str>) -> Result<Vec<Session>, SdkError> {
        Ok(Vec::new())
    }

    async fn session_abort(&self, _directory: &str, session_id: &str) -> Result<(), SdkError> {
        self.aborted.lock().unwrap().push(session_id.to_string());
        Ok(())
    }

    async fn session_prompt(
        &self,
        _request: oc_acp::sdk::PromptRequest,
    ) -> Result<oc_acp::sdk::AssistantMessage, SdkError> {
        Ok(assistant_message(self.prompt_error.lock().unwrap().clone()))
    }

    async fn session_command(
        &self,
        _request: oc_acp::sdk::CommandRequest,
    ) -> Result<oc_acp::sdk::AssistantMessage, SdkError> {
        Ok(assistant_message(None))
    }

    async fn session_summarize(
        &self,
        _request: oc_acp::sdk::SummarizeRequest,
    ) -> Result<bool, SdkError> {
        Ok(true)
    }

    async fn session_fork(&self, _directory: &str, _session_id: &str) -> Result<Session, SdkError> {
        Ok(Session {
            id: "forked".into(),
            directory: "/tmp".into(),
            title: "Forked".into(),
            time: SessionTime {
                created: 6,
                updated: 6,
            },
        })
    }

    async fn config_providers(
        &self,
        _directory: &str,
    ) -> Result<oc_acp::sdk::ConfigProviders, SdkError> {
        Ok(oc_acp::sdk::ConfigProviders {
            providers: self.providers.clone(),
        })
    }

    async fn config_get(&self, _directory: &str) -> Result<Config, SdkError> {
        Ok(self.config.clone())
    }

    async fn app_agents(&self, _directory: &str) -> Result<Vec<AgentInfo>, SdkError> {
        Ok(self.agents.clone())
    }

    async fn app_skills(&self, _directory: &str) -> Result<Vec<oc_acp::sdk::SkillInfo>, SdkError> {
        Ok(Vec::new())
    }

    async fn command_list(
        &self,
        _directory: &str,
    ) -> Result<Vec<oc_acp::sdk::CommandInfo>, SdkError> {
        Ok(Vec::new())
    }

    async fn mcp_add(
        &self,
        _directory: &str,
        _name: &str,
        _config: Value,
    ) -> Result<Value, SdkError> {
        Ok(Value::Null)
    }

    async fn permission_reply(
        &self,
        _request_id: &str,
        _reply: &str,
        _directory: &str,
    ) -> Result<bool, SdkError> {
        Ok(true)
    }
}

fn assistant_message(error: Option<Value>) -> oc_acp::sdk::AssistantMessage {
    oc_acp::sdk::AssistantMessage {
        id: "m1".into(),
        session_id: "s1".into(),
        role: "assistant".into(),
        provider_id: "anthropic".into(),
        model_id: "claude-sonnet-4".into(),
        mode: Some("build".into()),
        agent: Some("build".into()),
        cost: 0.0,
        tokens: oc_acp::sdk::Tokens {
            input: 0,
            output: 0,
            reasoning: 0,
            cache: oc_acp::sdk::CacheTokens { read: 0, write: 0 },
        },
        variant: None,
        error,
        path: None,
        model: None,
    }
}

/// A connection that records every `session/update` it is asked to send.
#[derive(Default)]
struct RecordingConnection {
    updates: Mutex<Vec<(String, SessionUpdate)>>,
}

#[async_trait::async_trait]
impl AgentSideConnection for RecordingConnection {
    async fn session_update(&self, session_id: &str, update: SessionUpdate) -> Result<(), ()> {
        self.updates
            .lock()
            .unwrap()
            .push((session_id.to_string(), update));
        Ok(())
    }

    async fn request_permission(
        &self,
        _request: oc_acp::types::RequestPermissionRequest,
    ) -> Result<oc_acp::types::RequestPermissionResponse, ()> {
        Ok(oc_acp::types::RequestPermissionResponse {
            outcome: oc_acp::types::RequestPermissionOutcome::Selected(
                oc_acp::types::SelectedPermissionOutcome {
                    option_id: "once".into(),
                },
            ),
        })
    }

    async fn write_text_file(
        &self,
        _request: oc_acp::types::WriteTextFileRequest,
    ) -> Result<(), ()> {
        Ok(())
    }
}

fn text_part_updated() -> Event {
    Event::MessagePartUpdated {
        id: "e1".into(),
        properties: oc_acp::sdk::MessagePartUpdatedProperties {
            session_id: "s1".into(),
            part: Part::Text(oc_acp::sdk::TextPart {
                id: "p-text".into(),
                session_id: "s1".into(),
                message_id: "m1".into(),
                text: "hello from the agent".into(),
                synthetic: None,
                ignored: None,
                metadata: None,
            }),
        },
    }
}

fn text_part_delta() -> Event {
    Event::MessagePartDelta {
        id: "e2".into(),
        properties: oc_acp::sdk::MessagePartDeltaProperties {
            session_id: "s1".into(),
            message_id: "m1".into(),
            part_id: "p-text".into(),
            field: "text".into(),
            delta: "hello from the agent".into(),
        },
    }
}

fn tool_part_updated() -> Event {
    Event::MessagePartUpdated {
        id: "e3".into(),
        properties: oc_acp::sdk::MessagePartUpdatedProperties {
            session_id: "s1".into(),
            part: Part::Tool(ToolPart {
                id: "p-tool".into(),
                session_id: "s1".into(),
                message_id: "m1".into(),
                call_id: "call-1".into(),
                tool: "read".into(),
                state: ToolState::Completed(ToolStateCompleted {
                    input: Map::from_iter([(
                        "filePath".into(),
                        Value::String("/tmp/notes.txt".into()),
                    )]),
                    output: "read ok".into(),
                    title: "Read notes".into(),
                    metadata: Map::new(),
                    attachments: None,
                }),
                metadata: None,
            }),
        },
    }
}

fn idle_event() -> Event {
    Event::SessionStatus {
        id: "e4".into(),
        properties: SessionStatusProperties {
            session_id: "s1".into(),
            status: SessionStatus {
                kind: "idle".into(),
            },
        },
    }
}

fn initialize_request() -> InitializeRequest {
    InitializeRequest {
        protocol_version: 1,
        client_capabilities: None,
        client_info: None,
        _meta: None,
    }
}

fn new_session_request() -> NewSessionRequest {
    NewSessionRequest {
        cwd: "/tmp".into(),
        mcp_servers: vec![],
        additional_directories: None,
        _meta: None,
    }
}

fn prompt_request(message_id: &str) -> PromptRequest {
    PromptRequest {
        session_id: "s1".into(),
        prompt: vec![ContentBlock::Text(TextContent {
            text: "transcribe this".into(),
            annotations: None,
        })],
        message_id: Some(message_id.into()),
        _meta: None,
    }
}

async fn wait_until(predicate: impl Fn() -> bool) {
    for _ in 0..200 {
        if predicate() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for condition");
}

#[tokio::test]
async fn full_transcript_streams_text_tool_then_idle() {
    let sdk = Arc::new(ScriptedSdk::default());
    let connection = Arc::new(RecordingConnection::default());
    let service = Service::make(ServiceInput::new(sdk.clone()).connection(connection.clone()));

    // initialize
    let init = service.initialize(&initialize_request()).await.unwrap();
    assert_eq!(init.protocol_version, 1);
    assert_eq!(init.agent_info.name, "OpenCode");

    // session/new
    let created = service.new_session(&new_session_request()).await.unwrap();
    assert_eq!(created.session_id, "s1");

    // Queue the streaming transcript: text part updated → text delta →
    // tool part updated → session idle.
    // Store the assistant message the deltas belong to so message lookups
    // resolve while streaming (mirrors a real persisted provider message).
    *sdk.assistant_message.lock().unwrap() = Some(SessionMessageResponse {
        info: oc_acp::sdk::Message::Assistant(assistant_message(None)),
        parts: vec![Part::Text(oc_acp::sdk::TextPart {
            id: "p-text".into(),
            session_id: "s1".into(),
            message_id: "m1".into(),
            text: "hello from the agent".into(),
            synthetic: None,
            ignored: None,
            metadata: None,
        })],
    });
    sdk.queue(vec![
        text_part_updated(),
        text_part_delta(),
        tool_part_updated(),
        idle_event(),
    ]);

    let response = service.prompt(&prompt_request("user-1")).await.unwrap();
    assert_eq!(response.stop_reason, StopReason::EndTurn);

    // Now the connection should have recorded the streamed session/update
    // notifications in order: text chunk, then tool call + tool call update.
    wait_until(|| connection.updates.lock().unwrap().len() >= 3).await;

    let updates = connection.updates.lock().unwrap();
    let kinds: Vec<&str> = updates
        .iter()
        .map(|(_, update)| match update {
            SessionUpdate::UserMessageChunk(_) => "user_message_chunk",
            SessionUpdate::AgentThoughtChunk(_) => "agent_thought_chunk",
            SessionUpdate::AgentMessageChunk(_) => "agent_message_chunk",
            SessionUpdate::ToolCall(_) => "tool_call",
            SessionUpdate::ToolCallUpdate(_) => "tool_call_update",
            SessionUpdate::AvailableCommandsUpdate(_) => "available_commands_update",
            SessionUpdate::UsageUpdate(_) => "usage_update",
        })
        .collect();

    // The new_session call also sends an available_commands_update; filter it
    // out so the streaming transcript order is what we assert.
    let streaming: Vec<&str> = kinds
        .iter()
        .copied()
        .filter(|kind| *kind != "available_commands_update")
        .collect();
    assert_eq!(
        streaming,
        vec![
            "agent_message_chunk", // text delta
            "tool_call",           // tool start
            "tool_call_update",    // tool completed
        ]
    );

    // The text chunk carries the streamed text.
    let text_chunk = updates
        .iter()
        .find(|(_, update)| matches!(update, SessionUpdate::AgentMessageChunk(_)))
        .unwrap()
        .1
        .clone();
    let SessionUpdate::AgentMessageChunk(chunk) = text_chunk else {
        unreachable!();
    };
    assert_eq!(chunk.message_id.as_deref(), Some("m1"));
    assert_eq!(
        chunk.content,
        ContentBlock::Text(TextContent {
            text: "hello from the agent".into(),
            annotations: None,
        })
    );

    // The tool call update reflects the completed tool result.
    let tool_update = updates
        .iter()
        .find(|(_, update)| matches!(update, SessionUpdate::ToolCallUpdate(_)))
        .unwrap()
        .1
        .clone();
    let value = serde_json::to_value(tool_update).unwrap();
    assert_eq!(value["toolCallId"], "call-1");
    assert_eq!(value["content"][0]["content"]["text"], "read ok");
}

#[tokio::test]
async fn transcripts_stop_with_cancelled_on_aborted_message() {
    let sdk = Arc::new(ScriptedSdk::default());
    let service = Service::make(ServiceInput::new(sdk.clone()));
    service.new_session(&new_session_request()).await.unwrap();

    // Cancel aborts the backing session.
    service
        .cancel(&oc_acp::types::CancelNotification {
            session_id: "s1".into(),
            _meta: None,
        })
        .await
        .unwrap();
    assert_eq!(*sdk.aborted.lock().unwrap(), vec!["s1"]);

    // A subsequent prompt whose provider message was aborted maps to
    // stop_reason `cancelled`.
    *sdk.prompt_error.lock().unwrap() = Some(json!({ "name": "MessageAbortedError" }));
    let response = service
        .prompt(&prompt_request("user-cancel"))
        .await
        .unwrap();
    assert_eq!(response.stop_reason, StopReason::Cancelled);
    assert_eq!(response.user_message_id.as_deref(), Some("user-cancel"));
}
