#![allow(clippy::all)]
//! End-to-end wire golden tests for the ACP service, driven by an in-memory
//! fake opencode SDK.

use std::sync::{Arc, Mutex};

use futures::stream::BoxStream;
use futures::StreamExt;
use oc_acp::connection::AgentSideConnection;
use oc_acp::event::{self, StartInput};
use oc_acp::permission::Handler as PermissionHandler;
use oc_acp::sdk::{
    AgentInfo, CommandInfo, Config, ConfigProviders, Event, ModelInfo, ModelLimit, OpencodeClient,
    PermissionAskedProperties, PermissionTool, ProviderInfo, SdkError, Session,
    SessionCreateRequest, SessionMessageResponse, SessionTime, SkillInfo, Tokens,
};
use oc_acp::service::{Service, ServiceInput};
use oc_acp::session::{Service as SessionService, StoreInput};
use oc_acp::types::{
    CancelNotification, InitializeRequest, NewSessionRequest, PromptRequest, RequestError,
    SessionUpdate, SetSessionConfigOptionRequest,
};
use serde_json::{Map, Value};

struct FakeSdk {
    providers: Vec<ProviderInfo>,
    agents: Vec<AgentInfo>,
    commands: Vec<CommandInfo>,
    skills: Vec<SkillInfo>,
    config: Config,
    created: Mutex<Vec<SessionCreateRequest>>,
    prompts: Mutex<Vec<oc_acp::sdk::PromptRequest>>,
    prompt_error: Mutex<Option<Value>>,
    idle_event_stream: Mutex<bool>,
    prompt_started: Arc<tokio::sync::Notify>,
    messages: Mutex<Vec<SessionMessageResponse>>,
    replies: Mutex<Vec<(String, String, String)>>,
    mcp_adds: Mutex<Vec<(String, String, Value)>>,
    aborted: Mutex<Vec<String>>,
}

const PROMPT_TRANSCRIPT_FIXTURE: &str = r#"
[
  {"type":"text","text":"explain this","annotations":{"audience":["assistant"]}},
  {"type":"image","mimeType":"image/png","data":"AQI=","uri":"file:///tmp/diagram.png"},
  {"type":"resource","resource":{"text":"fn main() {}","uri":"file:///tmp/main.rs#L7","mimeType":"text/plain"}}
]
"#;

impl Default for FakeSdk {
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
            commands: vec![],
            skills: vec![],
            config: Config {
                model: Some("anthropic/claude-sonnet-4".into()),
            },
            created: Mutex::new(Vec::new()),
            prompts: Mutex::new(Vec::new()),
            prompt_error: Mutex::new(None),
            idle_event_stream: Mutex::new(false),
            prompt_started: Arc::new(tokio::sync::Notify::new()),
            messages: Mutex::new(Vec::new()),
            replies: Mutex::new(Vec::new()),
            mcp_adds: Mutex::new(Vec::new()),
            aborted: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl OpencodeClient for FakeSdk {
    fn global_event(&self) -> BoxStream<'static, Option<Event>> {
        if *self.idle_event_stream.lock().unwrap() {
            let prompt_started = self.prompt_started.clone();
            return futures::stream::once(async move {
                prompt_started.notified().await;
                Some(Event::SessionStatus {
                    id: "status-1".into(),
                    properties: oc_acp::sdk::SessionStatusProperties {
                        session_id: "s1".into(),
                        status: oc_acp::sdk::SessionStatus {
                            kind: "idle".into(),
                        },
                    },
                })
            })
            .boxed();
        }
        futures::stream::empty().boxed()
    }

    async fn session_create(&self, request: SessionCreateRequest) -> Result<Session, SdkError> {
        self.created.lock().unwrap().push(request);
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
        Ok(self.messages.lock().unwrap().clone())
    }

    async fn session_message(
        &self,
        _directory: &str,
        _session_id: &str,
        message_id: &str,
    ) -> Result<SessionMessageResponse, SdkError> {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .find(|message| message.info.id() == message_id)
            .cloned()
            .ok_or(Value::Null)
    }

    async fn session_list(&self, _directory: Option<&str>) -> Result<Vec<Session>, SdkError> {
        Ok(vec![Session {
            id: "s2".into(),
            directory: "/tmp".into(),
            title: "Listed".into(),
            time: SessionTime {
                created: 5,
                updated: 5,
            },
        }])
    }

    async fn session_abort(&self, _directory: &str, session_id: &str) -> Result<(), SdkError> {
        self.aborted.lock().unwrap().push(session_id.to_string());
        Ok(())
    }

    async fn session_prompt(
        &self,
        request: oc_acp::sdk::PromptRequest,
    ) -> Result<oc_acp::sdk::AssistantMessage, SdkError> {
        self.prompts.lock().unwrap().push(request);
        self.prompt_started.notify_waiters();
        let mut response = assistant_message();
        response.error = self.prompt_error.lock().unwrap().clone();
        Ok(response)
    }

    async fn session_command(
        &self,
        _request: oc_acp::sdk::CommandRequest,
    ) -> Result<oc_acp::sdk::AssistantMessage, SdkError> {
        Ok(assistant_message())
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

    async fn config_providers(&self, _directory: &str) -> Result<ConfigProviders, SdkError> {
        Ok(ConfigProviders {
            providers: self.providers.clone(),
        })
    }

    async fn config_get(&self, _directory: &str) -> Result<Config, SdkError> {
        Ok(self.config.clone())
    }

    async fn app_agents(&self, _directory: &str) -> Result<Vec<AgentInfo>, SdkError> {
        Ok(self.agents.clone())
    }

    async fn app_skills(&self, _directory: &str) -> Result<Vec<SkillInfo>, SdkError> {
        Ok(self.skills.clone())
    }

    async fn command_list(&self, _directory: &str) -> Result<Vec<CommandInfo>, SdkError> {
        Ok(self.commands.clone())
    }

    async fn mcp_add(&self, directory: &str, name: &str, config: Value) -> Result<Value, SdkError> {
        self.mcp_adds
            .lock()
            .unwrap()
            .push((directory.to_string(), name.to_string(), config));
        Ok(Value::Null)
    }

    async fn permission_reply(
        &self,
        request_id: &str,
        reply: &str,
        directory: &str,
    ) -> Result<bool, SdkError> {
        self.replies.lock().unwrap().push((
            request_id.to_string(),
            reply.to_string(),
            directory.to_string(),
        ));
        Ok(true)
    }
}

fn assistant_message() -> oc_acp::sdk::AssistantMessage {
    oc_acp::sdk::AssistantMessage {
        id: "m1".into(),
        session_id: "s1".into(),
        role: "assistant".into(),
        provider_id: "anthropic".into(),
        model_id: "claude-sonnet-4".into(),
        mode: Some("build".into()),
        agent: Some("build".into()),
        cost: 0.01,
        tokens: Tokens {
            input: 100,
            output: 50,
            reasoning: 0,
            cache: oc_acp::sdk::CacheTokens { read: 0, write: 0 },
        },
        variant: None,
        error: None,
        path: None,
        model: None,
    }
}

#[tokio::test]
async fn initialize_response_golden() {
    let sdk = Arc::new(FakeSdk::default());
    let service = Service::make(ServiceInput::new(sdk));
    let response = service
        .initialize(&InitializeRequest {
            protocol_version: 1,
            client_capabilities: None,
            client_info: None,
            _meta: None,
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&response).unwrap(),
        r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"mcpCapabilities":{"http":true,"sse":true},"promptCapabilities":{"embeddedContext":true,"image":true},"sessionCapabilities":{"close":{},"fork":{},"list":{},"resume":{}}},"authMethods":[{"description":"Run `opencode auth login` in the terminal","name":"Login with opencode","id":"opencode-login"}],"agentInfo":{"name":"OpenCode","version":"local"}}"#
    );
}

#[tokio::test]
async fn initialize_terminal_auth_golden() {
    let sdk = Arc::new(FakeSdk::default());
    let service = Service::make(ServiceInput::new(sdk));
    let mut meta = Map::new();
    meta.insert("terminal-auth".to_string(), Value::Bool(true));
    let response = service
        .initialize(&InitializeRequest {
            protocol_version: 1,
            client_capabilities: Some(oc_acp::types::ClientCapabilities { _meta: Some(meta) }),
            client_info: None,
            _meta: None,
        })
        .await
        .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        value["authMethods"][0]["_meta"]["terminal-auth"],
        serde_json::json!({
            "command": "opencode",
            "args": ["auth", "login"],
            "label": "OpenCode Login"
        })
    );
}

#[tokio::test]
async fn new_session_golden() {
    let sdk = Arc::new(FakeSdk::default());
    let service = Service::make(ServiceInput::new(sdk.clone()));
    let response = service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: Some(vec!["/workspace/shared".into()]),
            _meta: None,
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&response).unwrap(),
        r#"{"sessionId":"s1","configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"anthropic/claude-sonnet-4","options":[{"value":"anthropic/claude-sonnet-4","name":"anthropic/Claude Sonnet 4"}]},{"id":"mode","name":"Session Mode","category":"mode","type":"select","currentValue":"build","options":[{"value":"build","name":"build","description":"The default mode"}]}]}"#
    );
    let created = sdk.created.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].agent.as_deref(), Some("build"));
    assert_eq!(created[0].model.provider_id, "anthropic");
    assert_eq!(created[0].model.id, "claude-sonnet-4");
}

#[tokio::test]
async fn prompt_transcript_preserves_provider_and_filesystem_content() {
    let sdk = Arc::new(FakeSdk::default());
    let service = Service::make(ServiceInput::new(sdk.clone()));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: Some(vec!["/workspace/shared".into()]),
            _meta: None,
        })
        .await
        .unwrap();

    let prompt: Vec<oc_acp::types::ContentBlock> =
        serde_json::from_str(PROMPT_TRANSCRIPT_FIXTURE).unwrap();
    let response = service
        .prompt(&PromptRequest {
            session_id: "s1".into(),
            prompt,
            message_id: Some("user-1".into()),
            _meta: None,
        })
        .await
        .unwrap();

    assert_eq!(response.stop_reason, oc_acp::types::StopReason::EndTurn);
    let prompts = sdk.prompts.lock().unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].model.provider_id, "anthropic");
    assert_eq!(prompts[0].model.model_id, "claude-sonnet-4");
    let created = sdk.created.lock().unwrap();
    assert_eq!(created[0].directory, "/tmp");
    assert_eq!(
        serde_json::to_value(&prompts[0].parts).unwrap(),
        serde_json::json!([
            {"type":"text","text":"explain this","synthetic":true},
            {"type":"file","url":"data:image/png;base64,AQI=","filename":"diagram.png","mime":"image/png"},
            {"type":"text","text":"[/tmp/main.rs:7]\nfn main() {}"}
        ])
    );
}

#[tokio::test]
async fn prompt_waits_for_session_idle_status() {
    let sdk = Arc::new(FakeSdk::default());
    *sdk.idle_event_stream.lock().unwrap() = true;
    let connection = Arc::new(RecordingConnection::default());
    let service = Service::make(ServiceInput::new(sdk.clone()).connection(connection));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();
    let response = service
        .prompt(&PromptRequest {
            session_id: "s1".into(),
            prompt: vec![oc_acp::types::ContentBlock::Text(
                oc_acp::types::TextContent {
                    text: "wait for transcript".into(),
                    annotations: None,
                },
            )],
            message_id: Some("user-2".into()),
            _meta: None,
        })
        .await
        .unwrap();
    assert_eq!(response.stop_reason, oc_acp::types::StopReason::EndTurn);
}

#[tokio::test]
async fn prompt_golden_with_usage() {
    let sdk = Arc::new(FakeSdk::default());
    // Provide a session with assistant messages so usage can be computed.
    let mut messages = Vec::new();
    messages.push(SessionMessageResponse {
        info: oc_acp::sdk::Message::Assistant(assistant_message()),
        parts: vec![],
    });
    *sdk.messages.lock().unwrap() = messages;

    let service = Service::make(ServiceInput::new(sdk.clone()));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();
    let response = service
        .prompt(&PromptRequest {
            session_id: "s1".into(),
            prompt: vec![oc_acp::types::ContentBlock::Text(
                oc_acp::types::TextContent {
                    text: "hi".into(),
                    annotations: None,
                },
            )],
            message_id: Some("msg-1".into()),
            _meta: None,
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&response).unwrap(),
        r#"{"stopReason":"end_turn","usage":{"inputTokens":100,"outputTokens":50,"totalTokens":150},"userMessageId":"msg-1","_meta":{}}"#
    );
}

#[tokio::test]
async fn prompt_maps_nested_provider_auth_error() {
    let sdk = Arc::new(FakeSdk::default());
    *sdk.prompt_error.lock().unwrap() = Some(serde_json::json!({
        "name": "HttpError",
        "data": {
            "error": {
                "name": "LoadAPIKeyError",
                "data": { "providerID": "anthropic" }
            }
        }
    }));
    let service = Service::make(ServiceInput::new(sdk));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();
    let error = service
        .prompt(&PromptRequest {
            session_id: "s1".into(),
            prompt: vec![oc_acp::types::ContentBlock::Text(
                oc_acp::types::TextContent {
                    text: "provider auth".into(),
                    annotations: None,
                },
            )],
            message_id: Some("user-auth".into()),
            _meta: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        oc_acp::error::ACPError::AuthRequired {
            provider_id: Some(provider_id)
        } if provider_id == "anthropic"
    ));
}

#[tokio::test]
async fn prompt_refreshes_provider_error_persisted_before_idle() {
    let sdk = Arc::new(FakeSdk::default());
    *sdk.idle_event_stream.lock().unwrap() = true;
    let mut final_message = assistant_message();
    final_message.error = Some(serde_json::json!({
        "name": "ProviderAuthError",
        "data": { "providerID": "anthropic" }
    }));
    *sdk.messages.lock().unwrap() = vec![SessionMessageResponse {
        info: oc_acp::sdk::Message::Assistant(final_message),
        parts: vec![],
    }];
    let connection = Arc::new(RecordingConnection::default());
    let service = Service::make(ServiceInput::new(sdk.clone()).connection(connection));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();

    let error = service
        .prompt(&PromptRequest {
            session_id: "s1".into(),
            prompt: vec![oc_acp::types::ContentBlock::Text(
                oc_acp::types::TextContent {
                    text: "provider auth after idle".into(),
                    annotations: None,
                },
            )],
            message_id: Some("user-auth-after-idle".into()),
            _meta: None,
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        oc_acp::error::ACPError::AuthRequired {
            provider_id: Some(provider_id)
        } if provider_id == "anthropic"
    ));
}

#[tokio::test]
async fn set_config_option_invalid_value() {
    let sdk = Arc::new(FakeSdk::default());
    let service = Service::make(ServiceInput::new(sdk.clone()));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();
    let result = service
        .set_session_config_option(&SetSessionConfigOptionRequest {
            session_id: "s1".into(),
            config_id: "model".into(),
            value: oc_acp::types::ConfigOptionValue::Boolean(true),
            r#type: Some("boolean".into()),
            _meta: None,
        })
        .await;
    assert!(matches!(
        result,
        Err(oc_acp::error::ACPError::InvalidConfigOption { .. })
    ));
}

#[tokio::test]
async fn cancel_aborts_backing_session() {
    let sdk = Arc::new(FakeSdk::default());
    let service = Service::make(ServiceInput::new(sdk.clone()));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();
    service
        .cancel(&CancelNotification {
            session_id: "s1".into(),
            _meta: None,
        })
        .await
        .unwrap();
    assert_eq!(*sdk.aborted.lock().unwrap(), vec!["s1"]);
}

#[tokio::test]
async fn permission_transcript_writes_file_metadata_patch() {
    let sdk = Arc::new(FakeSdk::default());
    let session = Arc::new(oc_acp::session::Service::new());
    let connection = Arc::new(RecordingConnection::default());
    let service = Service::make(ServiceInput {
        sdk: sdk.clone(),
        connection: Some(connection.clone()),
        session: Some(session.clone()),
        ..ServiceInput::new(sdk.clone())
    });
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();

    let path = std::env::temp_dir().join("opencode-acp-transcript-fixture.txt");
    std::fs::write(&path, "before\n").unwrap();
    let mut metadata = Map::new();
    metadata.insert(
        "files".into(),
        serde_json::json!([{
            "filePath": path.to_string_lossy(),
            "relativePath": "transcript-fixture.txt",
            "patch": "@@ -1,1 +1,1 @@\n-before\n+after\n"
        }]),
    );

    let handler = PermissionHandler::new(sdk.clone(), Some(connection.clone()), session);
    handler
        .handle(&PermissionAskedProperties {
            id: "permission-1".into(),
            session_id: "s1".into(),
            permission: "edit".into(),
            patterns: vec![],
            metadata,
            always: vec![],
            tool: Some(PermissionTool {
                message_id: "message-1".into(),
                call_id: "call-1".into(),
            }),
        })
        .await;

    let writes = connection.write_calls.lock().unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, "s1");
    assert_eq!(writes[0].1, path.to_string_lossy());
    assert_eq!(writes[0].2, "after\n");
    assert_eq!(sdk.replies.lock().unwrap()[0].1, "once");
    let _ = std::fs::remove_file(path);
}

/// A recording connection capturing `session/update` notifications.
struct RecordingConnection {
    updates: Mutex<Vec<(String, SessionUpdate)>>,
    write_calls: Mutex<Vec<(String, String, String)>>,
}

impl Default for RecordingConnection {
    fn default() -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            write_calls: Mutex::new(Vec::new()),
        }
    }
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
        request: oc_acp::types::WriteTextFileRequest,
    ) -> Result<(), ()> {
        self.write_calls
            .lock()
            .unwrap()
            .push((request.session_id, request.path, request.content));
        Ok(())
    }
}

#[tokio::test]
async fn permission_transcript_mediates_multi_file_write() {
    let sdk = Arc::new(FakeSdk::default());
    let connection = Arc::new(RecordingConnection::default());
    let sessions = Arc::new(SessionService::new());
    sessions
        .create(StoreInput {
            id: "s1".into(),
            cwd: "/tmp".into(),
            mcp_servers: None,
            created_at: Some(1),
            model: None,
            variant: None,
            mode_id: None,
        })
        .await;
    let handler = PermissionHandler::new(sdk.clone(), Some(connection.clone()), sessions);
    let mut metadata = Map::new();
    metadata.insert(
        "files".into(),
        serde_json::json!([{
            "filePath": "/__acp_fixture__/new.txt",
            "patch": "@@ -0,0 +1,2 @@\n+alpha\n+beta\n"
        }]),
    );
    handler
        .handle(&PermissionAskedProperties {
            id: "permission-1".into(),
            session_id: "s1".into(),
            permission: "edit".into(),
            patterns: vec![],
            metadata,
            always: vec![],
            tool: Some(PermissionTool {
                message_id: "m1".into(),
                call_id: "call-1".into(),
            }),
        })
        .await;

    assert_eq!(
        *connection.write_calls.lock().unwrap(),
        vec![(
            "s1".into(),
            "/__acp_fixture__/new.txt".into(),
            "alpha\nbeta".into()
        )]
    );
    assert_eq!(
        *sdk.replies.lock().unwrap(),
        vec![("permission-1".into(), "once".into(), "/tmp".into())]
    );
}

#[tokio::test]
async fn replay_transcript_emits_deterministic_filesystem_and_tool_updates() {
    let sdk = Arc::new(FakeSdk::default());
    let connection = Arc::new(RecordingConnection::default());
    let sessions = Arc::new(SessionService::new());
    sessions
        .create(StoreInput {
            id: "s1".into(),
            cwd: "/tmp".into(),
            mcp_servers: None,
            created_at: Some(1),
            model: None,
            variant: None,
            mode_id: None,
        })
        .await;
    let subscription = event::start(StartInput {
        sdk,
        connection: Some(connection.clone()),
        session: sessions,
    });

    subscription
        .replay_message(&SessionMessageResponse {
            info: oc_acp::sdk::Message::User(oc_acp::sdk::UserMessage {
                id: "user-1".into(),
                session_id: "s1".into(),
                role: "user".into(),
                model: None,
                agent: Some("build".into()),
            }),
            parts: vec![oc_acp::sdk::Part::Text(oc_acp::sdk::TextPart {
                id: "user-part".into(),
                session_id: "s1".into(),
                message_id: "user-1".into(),
                text: "hello".into(),
                synthetic: None,
                ignored: None,
                metadata: None,
            })],
        })
        .await;
    subscription
        .replay_message(&SessionMessageResponse {
            info: oc_acp::sdk::Message::Assistant(assistant_message()),
            parts: vec![
                oc_acp::sdk::Part::Reasoning(oc_acp::sdk::ReasoningPart {
                    id: "reasoning-1".into(),
                    session_id: "s1".into(),
                    message_id: "m1".into(),
                    text: "thinking".into(),
                    metadata: None,
                }),
                oc_acp::sdk::Part::Text(oc_acp::sdk::TextPart {
                    id: "text-1".into(),
                    session_id: "s1".into(),
                    message_id: "m1".into(),
                    text: "done".into(),
                    synthetic: None,
                    ignored: None,
                    metadata: None,
                }),
                oc_acp::sdk::Part::File(oc_acp::sdk::FilePart {
                    id: "file-1".into(),
                    session_id: "s1".into(),
                    message_id: "m1".into(),
                    mime: "text/plain".into(),
                    filename: Some("notes.txt".into()),
                    url: "file:///tmp/notes.txt".into(),
                }),
                oc_acp::sdk::Part::Tool(oc_acp::sdk::ToolPart {
                    id: "tool-part".into(),
                    session_id: "s1".into(),
                    message_id: "m1".into(),
                    call_id: "call-1".into(),
                    tool: "read".into(),
                    state: oc_acp::sdk::ToolState::Completed(oc_acp::sdk::ToolStateCompleted {
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
            ],
        })
        .await;
    subscription.stop();

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
    assert_eq!(
        kinds,
        vec![
            "user_message_chunk",
            "agent_thought_chunk",
            "agent_message_chunk",
            "agent_message_chunk",
            "tool_call",
            "tool_call_update"
        ]
    );
    assert_eq!(
        serde_json::to_value(&updates[3].1).unwrap()["content"],
        serde_json::json!({
            "type": "resource_link",
            "uri": "file:///tmp/notes.txt",
            "name": "notes.txt",
            "mimeType": "text/plain"
        })
    );
    assert_eq!(
        serde_json::to_value(&updates[5].1).unwrap()["content"][0]["content"]["text"],
        "read ok"
    );
}

#[tokio::test]
async fn new_session_sends_available_commands() {
    let sdk = Arc::new(FakeSdk::default());
    let connection = Arc::new(RecordingConnection::default());
    let service = Service::make(ServiceInput::new(sdk).connection(connection.clone()));
    service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let updates = connection.updates.lock().unwrap();
    assert!(updates
        .iter()
        .any(|(_, update)| matches!(update, SessionUpdate::AvailableCommandsUpdate(_))));
}

#[tokio::test]
async fn error_maps_to_request_error() {
    let sdk = Arc::new(FakeSdk::default());
    let service = Service::make(ServiceInput::new(sdk));
    let result = service
        .prompt(&PromptRequest {
            session_id: "missing".into(),
            prompt: vec![],
            message_id: None,
            _meta: None,
        })
        .await;
    assert!(matches!(
        result,
        Err(oc_acp::error::ACPError::SessionNotFound { .. })
    ));
    let request_error = oc_acp::error::to_request_error(&result.unwrap_err());
    assert_eq!(request_error.code, RequestError::INVALID_PARAMS);
}
