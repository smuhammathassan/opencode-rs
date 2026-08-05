//! End-to-end wire golden tests for the ACP service, driven by an in-memory
//! fake opencode SDK.

use std::sync::{Arc, Mutex};

use futures::stream::BoxStream;
use futures::StreamExt;
use oc_acp::connection::AgentSideConnection;
use oc_acp::sdk::{
    AgentInfo, CommandInfo, Config, ConfigProviders, Event, ModelInfo, ModelLimit, OpencodeClient,
    ProviderInfo, SdkError, Session, SessionCreateRequest, SessionMessageResponse, SessionTime,
    SkillInfo, Tokens,
};
use oc_acp::service::{Service, ServiceInput};
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
    messages: Mutex<Vec<SessionMessageResponse>>,
    replies: Mutex<Vec<(String, String, String)>>,
    mcp_adds: Mutex<Vec<(String, String, Value)>>,
    aborted: Mutex<Vec<String>>,
}

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
        _message_id: &str,
    ) -> Result<SessionMessageResponse, SdkError> {
        Err(Value::Null)
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
        _request: oc_acp::sdk::PromptRequest,
    ) -> Result<oc_acp::sdk::AssistantMessage, SdkError> {
        Ok(assistant_message())
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
    let service = Service::make(ServiceInput::new(sdk));
    let response = service
        .new_session(&NewSessionRequest {
            cwd: "/tmp".into(),
            mcp_servers: vec![],
            additional_directories: None,
            _meta: None,
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&response).unwrap(),
        r#"{"sessionId":"s1","configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"anthropic/claude-sonnet-4","options":[{"value":"anthropic/claude-sonnet-4","name":"anthropic/Claude Sonnet 4"}]},{"id":"mode","name":"Session Mode","category":"mode","type":"select","currentValue":"build","options":[{"value":"build","name":"build","description":"The default mode"}]}]}"#
    );
}

#[tokio::test]
async fn prompt_golden_with_usage() {
    let sdk = Arc::new(FakeSdk::default());
    // Provide a session with assistant messages so usage can be computed.
    let messages = vec![SessionMessageResponse {
        info: oc_acp::sdk::Message::Assistant(Box::new(assistant_message())),
        parts: vec![],
    }];
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
