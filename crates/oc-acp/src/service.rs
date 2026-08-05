//! The ACP service.
//!
//! From reference/packages/opencode/src/acp/service.ts. Implements the agent
//! side of the Agent Client Protocol over the opencode SDK.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use tokio::sync::Mutex;

use crate::config_option;
use crate::connection::AgentSideConnection;
use crate::content;
use crate::directory;
use crate::error::ACPError;
use crate::event;
use crate::sdk::{
    AssistantMessage, CommandInfo, CommandRequest as SdkCommandRequest, Message, ModelInfo,
    OpencodeClient, PromptPart, PromptRequest as SdkPromptRequest, ProviderInfo,
    SessionCreateModel, SessionCreateRequest, SessionMessageResponse, SummarizeRequest,
};
use crate::session;
use crate::types::{
    AgentCapabilities, AuthenticateRequest, AuthenticateResponse, AvailableCommand,
    AvailableCommandsUpdate, CancelNotification, CloseSessionRequest, CloseSessionResponse,
    ConfigOptionValue, ForkSessionRequest, ForkSessionResponse, Implementation, InitializeRequest,
    InitializeResponse, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest,
    LoadSessionResponse, McpCapabilities, McpServer, NewSessionRequest, NewSessionResponse,
    PromptCapabilities, PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SessionCapabilities, SessionCloseCapabilities, SessionConfigOption, SessionForkCapabilities,
    SessionInfo, SessionListCapabilities, SessionResumeCapabilities, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, SetSessionModelRequest, SetSessionModelResponse, StopReason,
};
use crate::usage;

/// The single supported authentication method id.
pub const AUTH_METHOD_ID: &str = "opencode-login";

/// The opencode agent version advertised during initialization. Mirrors
/// `InstallationVersion` from reference/packages/core/src/installation/version.ts.
///
/// TODO(integration): inject the real build-time version constant.
pub fn installation_version() -> String {
    std::env::var("OPENCODE_VERSION").unwrap_or_else(|_| "local".to_string())
}

/// Input to [`Service::make`].
pub struct ServiceInput {
    pub sdk: Arc<dyn OpencodeClient>,
    pub connection: Option<Arc<dyn AgentSideConnection>>,
    pub directory: Option<Arc<directory::Service>>,
    pub session: Option<Arc<session::Service>>,
    pub usage: Option<Arc<usage::Service>>,
    /// Callback invoked with the started event subscription so the transport
    /// can wire it into the connection lifecycle.
    pub event_subscription: Option<Box<dyn FnOnce(Arc<event::Subscription>) + Send>>,
}

impl ServiceInput {
    pub fn new(sdk: Arc<dyn OpencodeClient>) -> Self {
        Self {
            sdk,
            connection: None,
            directory: None,
            session: None,
            usage: None,
            event_subscription: None,
        }
    }

    /// Set the agent-side connection to the ACP client.
    pub fn connection(mut self, connection: Arc<dyn AgentSideConnection>) -> Self {
        self.connection = Some(connection);
        self
    }

    /// Set the callback invoked with the started event subscription.
    pub fn event_subscription(
        mut self,
        callback: Box<dyn FnOnce(Arc<event::Subscription>) + Send>,
    ) -> Self {
        self.event_subscription = Some(callback);
        self
    }
}

/// The ACP service.
pub struct Service {
    sdk: Arc<dyn OpencodeClient>,
    connection: Option<Arc<dyn AgentSideConnection>>,
    session: Arc<session::Service>,
    directory: Arc<directory::Service>,
    usage: Arc<usage::Service>,
    registered_mcp: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    session_snapshots: Mutex<HashMap<String, directory::Snapshot>>,
    events: Option<Arc<event::Subscription>>,
}

/// `make` from reference/packages/opencode/src/acp/service.ts.
impl Service {
    pub fn make(input: ServiceInput) -> Self {
        let sdk = input.sdk;
        let connection = input.connection;
        let session = input
            .session
            .unwrap_or_else(|| Arc::new(session::Service::new()));
        let directory = input.directory.unwrap_or_else(|| {
            Arc::new(directory::Service::new(Box::new(SdkDirectoryLoader {
                sdk: sdk.clone(),
            })))
        });
        let usage = input
            .usage
            .unwrap_or_else(|| usage::Service::make(sdk.clone()));

        let events = if connection.is_some() {
            Some(event::start(event::StartInput {
                sdk: sdk.clone(),
                connection: connection.clone(),
                session: session.clone(),
            }))
        } else {
            None
        };
        if let Some(events) = &events {
            if let Some(callback) = input.event_subscription {
                callback(events.clone());
            }
        }

        Self {
            sdk,
            connection,
            session,
            directory,
            usage,
            registered_mcp: Arc::new(Mutex::new(HashMap::new())),
            session_snapshots: Mutex::new(HashMap::new()),
            events,
        }
    }

    /// `initialize` from reference/packages/opencode/src/acp/service.ts.
    pub async fn initialize(
        &self,
        params: &InitializeRequest,
    ) -> Result<InitializeResponse, ACPError> {
        let started = std::time::Instant::now();
        let mut auth_method = crate::types::AuthMethod {
            description: Some("Run `opencode auth login` in the terminal".into()),
            name: "Login with opencode".into(),
            id: AUTH_METHOD_ID.into(),
            _meta: None,
        };
        let terminal_auth = params
            .client_capabilities
            .as_ref()
            .and_then(|capabilities| capabilities._meta.as_ref())
            .and_then(|meta| meta.get("terminal-auth"))
            .and_then(Value::as_bool);
        if terminal_auth == Some(true) {
            auth_method._meta = Some(Map::from_iter([(
                "terminal-auth".into(),
                json!({
                    "command": "opencode",
                    "args": ["auth", "login"],
                    "label": "OpenCode Login",
                }),
            )]));
        }

        let response = InitializeResponse {
            protocol_version: 1,
            agent_capabilities: AgentCapabilities {
                load_session: Some(true),
                mcp_capabilities: Some(McpCapabilities {
                    http: Some(true),
                    sse: Some(true),
                }),
                prompt_capabilities: Some(PromptCapabilities {
                    audio: None,
                    embedded_context: Some(true),
                    image: Some(true),
                }),
                session_capabilities: Some(SessionCapabilities {
                    close: Some(SessionCloseCapabilities {}),
                    fork: Some(SessionForkCapabilities {}),
                    list: Some(SessionListCapabilities {}),
                    resume: Some(SessionResumeCapabilities {}),
                }),
            },
            auth_methods: vec![auth_method],
            agent_info: Implementation {
                name: "OpenCode".into(),
                title: None,
                version: installation_version(),
            },
            _meta: None,
        };
        crate::profile::duration("acp.initialize", started, &[]);
        Ok(response)
    }

    /// `authenticate` from reference/packages/opencode/src/acp/service.ts.
    pub async fn authenticate(
        &self,
        params: &AuthenticateRequest,
    ) -> Result<AuthenticateResponse, ACPError> {
        if params.method_id != AUTH_METHOD_ID {
            return Err(ACPError::UnknownAuthMethod {
                method_id: params.method_id.clone(),
            });
        }
        Ok(AuthenticateResponse::default())
    }

    async fn directory_snapshot(&self, cwd: &str) -> Result<directory::Snapshot, ACPError> {
        let started = std::time::Instant::now();
        let snapshot = self.directory.get(cwd).await;
        crate::profile::duration("acp.directory.snapshot", started, &[]);
        snapshot
    }

    async fn config_snapshot(
        &self,
        state: &session::Info,
    ) -> Result<directory::Snapshot, ACPError> {
        if let Some(snapshot) = self.session_snapshots.lock().await.get(&state.id).cloned() {
            return Ok(snapshot);
        }
        let loaded = self.directory_snapshot(&state.cwd).await?;
        self.session_snapshots
            .lock()
            .await
            .insert(state.id.clone(), loaded.clone());
        Ok(loaded)
    }

    /// `newSession` from reference/packages/opencode/src/acp/service.ts.
    pub async fn new_session(
        &self,
        params: &NewSessionRequest,
    ) -> Result<NewSessionResponse, ACPError> {
        let started = std::time::Instant::now();
        let snapshot = self.directory_snapshot(&params.cwd).await?;
        let selected = select_default_model(&snapshot);
        let variant = select_variant(&snapshot, &selected);
        let mode_id = mode_id_for(&snapshot);
        let created = self
            .profiled_request(
                "acp.newSession.session.create",
                self.sdk.session_create(SessionCreateRequest {
                    directory: params.cwd.clone(),
                    agent: mode_id.clone(),
                    model: SessionCreateModel {
                        provider_id: selected.provider_id.clone(),
                        id: selected.model_id.clone(),
                        variant: variant.clone(),
                    },
                }),
                Some("session"),
            )
            .await?;
        let state = self
            .session
            .create(session::StoreInput {
                id: created.id,
                cwd: params.cwd.clone(),
                mcp_servers: Some(params.mcp_servers.clone()),
                created_at: None,
                model: Some(session::SelectedModel::from(&selected)),
                variant: variant.clone(),
                mode_id: mode_id.clone(),
            })
            .await;
        self.session_snapshots
            .lock()
            .await
            .insert(state.id.clone(), snapshot.clone());
        self.register_mcp_servers(&params.cwd, &state.id, &params.mcp_servers)
            .await;
        self.send_available_commands(&state.id, &snapshot);
        let model = state
            .model
            .as_ref()
            .map(config_option::ModelRef::from)
            .unwrap_or_else(|| config_option::ModelRef::from(&selected));
        crate::profile::duration("acp.newSession", started, &[]);
        Ok(NewSessionResponse {
            session_id: state.id,
            config_options: config_options(
                &snapshot,
                &ConfigState {
                    model,
                    variant: state.variant.as_deref(),
                    mode_id: state.mode_id.as_deref(),
                },
            ),
        })
    }

    /// `loadSession` from reference/packages/opencode/src/acp/service.ts.
    pub async fn load_session(
        &self,
        params: &LoadSessionRequest,
    ) -> Result<LoadSessionResponse, ACPError> {
        let snapshot = self.directory_snapshot(&params.cwd).await?;
        self.request(
            self.sdk.session_get(&params.cwd, &params.session_id),
            Some("session"),
        )
        .await?;
        let messages = self
            .request(
                self.sdk
                    .session_messages(&params.cwd, &params.session_id, None),
                Some("session"),
            )
            .await?;
        let restored = restore_from_messages(&messages);
        let model = restored
            .model
            .as_ref()
            .map(directory::DefaultModel::from)
            .unwrap_or_else(|| select_default_model(&snapshot));
        let state = self
            .session
            .load(session::StoreInput {
                id: params.session_id.clone(),
                cwd: params.cwd.clone(),
                mcp_servers: Some(params.mcp_servers.clone()),
                created_at: None,
                model: Some(session::SelectedModel::from(&model)),
                variant: restored
                    .variant
                    .clone()
                    .or_else(|| select_variant(&snapshot, &model)),
                mode_id: restored.mode_id.clone().or_else(|| mode_id_for(&snapshot)),
            })
            .await;
        self.session_snapshots
            .lock()
            .await
            .insert(state.id.clone(), snapshot.clone());
        self.register_mcp_servers(&params.cwd, &state.id, &params.mcp_servers)
            .await;
        self.send_available_commands(&state.id, &snapshot);
        self.replay_messages(&messages).await;
        let model = state
            .model
            .as_ref()
            .map(config_option::ModelRef::from)
            .unwrap_or_else(|| config_option::ModelRef::from(&model));
        Ok(LoadSessionResponse {
            config_options: config_options(
                &snapshot,
                &ConfigState {
                    model,
                    variant: state.variant.as_deref(),
                    mode_id: state.mode_id.as_deref(),
                },
            ),
        })
    }

    /// `listSessions` from reference/packages/opencode/src/acp/service.ts.
    pub async fn list_sessions(
        &self,
        params: &ListSessionsRequest,
    ) -> Result<ListSessionsResponse, ACPError> {
        let cursor = params
            .cursor
            .as_ref()
            .and_then(|value| value.parse::<f64>().ok());
        let limit = 100;
        let sessions = self
            .request(
                self.sdk.session_list(params.cwd.as_deref()),
                Some("session"),
            )
            .await?;
        let server_entries: Vec<SessionInfo> = sessions
            .iter()
            .map(|item| SessionInfo {
                session_id: item.id.clone(),
                cwd: item.directory.clone(),
                title: Some(item.title.clone()),
                updated_at: Some(to_iso(&item.time.updated)),
            })
            .collect();
        let server_ids: HashSet<&str> = server_entries
            .iter()
            .map(|entry| entry.session_id.as_str())
            .collect();
        let live_entries: Vec<SessionInfo> = self
            .session
            .list(params.cwd.as_deref())
            .await
            .into_iter()
            .filter(|item| !server_ids.contains(item.id.as_str()))
            .map(|item| SessionInfo {
                session_id: item.id,
                cwd: item.cwd,
                title: None,
                updated_at: Some(to_iso(&item.created_at)),
            })
            .collect();
        let mut sorted = Vec::with_capacity(live_entries.len() + server_entries.len());
        sorted.extend(live_entries);
        sorted.extend(server_entries);
        sorted.sort_by(|a, b| {
            entry_timestamp(b)
                .partial_cmp(&entry_timestamp(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let filtered: Vec<SessionInfo> = match cursor {
            Some(cursor) if cursor.is_finite() => sorted
                .into_iter()
                .filter(|item| entry_timestamp(item) < cursor)
                .collect(),
            _ => sorted,
        };
        let page: Vec<SessionInfo> = filtered.iter().take(limit).cloned().collect();
        let last = page.last().cloned();
        Ok(ListSessionsResponse {
            sessions: page,
            next_cursor: if filtered.len() > limit {
                last.map(|last| format!("{}", entry_timestamp(&last) as u64))
            } else {
                None
            },
        })
    }

    /// `resumeSession` from reference/packages/opencode/src/acp/service.ts.
    pub async fn resume_session(
        &self,
        params: &ResumeSessionRequest,
    ) -> Result<ResumeSessionResponse, ACPError> {
        let snapshot = self.directory_snapshot(&params.cwd).await?;
        self.request(
            self.sdk.session_get(&params.cwd, &params.session_id),
            Some("session"),
        )
        .await?;
        let messages = self
            .request(
                self.sdk
                    .session_messages(&params.cwd, &params.session_id, Some(20)),
                Some("session"),
            )
            .await?;
        let restored = restore_from_messages(&messages);
        let model = restored
            .model
            .as_ref()
            .map(directory::DefaultModel::from)
            .unwrap_or_else(|| select_default_model(&snapshot));
        let mcp_servers = params.mcp_servers.clone().unwrap_or_default();
        let state = self
            .session
            .load(session::StoreInput {
                id: params.session_id.clone(),
                cwd: params.cwd.clone(),
                mcp_servers: Some(mcp_servers.clone()),
                created_at: None,
                model: Some(session::SelectedModel::from(&model)),
                variant: restored
                    .variant
                    .clone()
                    .or_else(|| select_variant(&snapshot, &model)),
                mode_id: restored.mode_id.clone().or_else(|| mode_id_for(&snapshot)),
            })
            .await;
        self.session_snapshots
            .lock()
            .await
            .insert(state.id.clone(), snapshot.clone());
        self.register_mcp_servers(&params.cwd, &state.id, &mcp_servers)
            .await;
        self.send_available_commands(&state.id, &snapshot);
        let model = state
            .model
            .as_ref()
            .map(config_option::ModelRef::from)
            .unwrap_or_else(|| config_option::ModelRef::from(&model));
        Ok(ResumeSessionResponse {
            config_options: config_options(
                &snapshot,
                &ConfigState {
                    model,
                    variant: state.variant.as_deref(),
                    mode_id: state.mode_id.as_deref(),
                },
            ),
        })
    }

    async fn abort_backing_session(&self, current: &session::Info) {
        if let Err(error) = self.sdk.session_abort(&current.cwd, &current.id).await {
            tracing::error!(
                "failed to abort ACP backing session: {error:?} sessionID={}",
                current.id
            );
        }
    }

    /// `closeSession` from reference/packages/opencode/src/acp/service.ts.
    pub async fn close_session(
        &self,
        params: &CloseSessionRequest,
    ) -> Result<CloseSessionResponse, ACPError> {
        let removed = self.session.remove(&params.session_id).await;
        self.registered_mcp.lock().await.remove(&params.session_id);
        self.session_snapshots
            .lock()
            .await
            .remove(&params.session_id);
        let Some(removed) = removed else {
            return Ok(CloseSessionResponse::default());
        };
        self.abort_backing_session(&removed).await;
        Ok(CloseSessionResponse::default())
    }

    /// `cancel` from reference/packages/opencode/src/acp/service.ts.
    pub async fn cancel(&self, params: &CancelNotification) -> Result<(), ACPError> {
        let current = self.session.get(&params.session_id).await?;
        self.abort_backing_session(&current).await;
        Ok(())
    }

    /// `forkSession` from reference/packages/opencode/src/acp/service.ts.
    pub async fn fork_session(
        &self,
        params: &ForkSessionRequest,
    ) -> Result<ForkSessionResponse, ACPError> {
        let snapshot = self.directory_snapshot(&params.cwd).await?;
        let forked = self
            .request(
                self.sdk.session_fork(&params.cwd, &params.session_id),
                Some("session"),
            )
            .await?;
        let messages = self
            .request(
                self.sdk.session_messages(&params.cwd, &forked.id, Some(20)),
                Some("session"),
            )
            .await?;
        let restored = restore_from_messages(&messages);
        let model = restored
            .model
            .as_ref()
            .map(directory::DefaultModel::from)
            .unwrap_or_else(|| select_default_model(&snapshot));
        let mcp_servers = params.mcp_servers.clone().unwrap_or_default();
        let state = self
            .session
            .load(session::StoreInput {
                id: forked.id.clone(),
                cwd: params.cwd.clone(),
                mcp_servers: Some(mcp_servers.clone()),
                created_at: None,
                model: Some(session::SelectedModel::from(&model)),
                variant: restored
                    .variant
                    .clone()
                    .or_else(|| select_variant(&snapshot, &model)),
                mode_id: restored.mode_id.clone().or_else(|| mode_id_for(&snapshot)),
            })
            .await;
        self.session_snapshots
            .lock()
            .await
            .insert(state.id.clone(), snapshot.clone());
        self.register_mcp_servers(&params.cwd, &state.id, &mcp_servers)
            .await;
        self.send_available_commands(&state.id, &snapshot);
        self.replay_messages(&messages).await;
        let model = state
            .model
            .as_ref()
            .map(config_option::ModelRef::from)
            .unwrap_or_else(|| config_option::ModelRef::from(&model));
        Ok(ForkSessionResponse {
            session_id: state.id,
            config_options: config_options(
                &snapshot,
                &ConfigState {
                    model,
                    variant: state.variant.as_deref(),
                    mode_id: state.mode_id.as_deref(),
                },
            ),
        })
    }

    /// `setSessionConfigOption` from reference/packages/opencode/src/acp/service.ts.
    pub async fn set_session_config_option(
        &self,
        params: &SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, ACPError> {
        let current = self.session.get(&params.session_id).await?;
        let snapshot = self.config_snapshot(&current).await?;
        let ConfigOptionValue::ValueId(value) = &params.value else {
            return Err(ACPError::InvalidConfigOption {
                config_id: params.config_id.clone(),
            });
        };

        if params.config_id == "model" {
            let selected = self.parse_selected_model(&snapshot, value).await?;
            let default_model = directory::DefaultModel::from(&selected.model);
            let variant = selected
                .variant
                .clone()
                .or_else(|| select_variant(&snapshot, &default_model));
            let variant = if directory::variants(&snapshot, &default_model).is_some() {
                variant
            } else {
                None
            };
            self.session
                .set_variant(&params.session_id, variant)
                .await?;
            let state = self
                .session
                .set_model(
                    &params.session_id,
                    Some(session::SelectedModel::from(&selected.model)),
                )
                .await?;
            let model = state
                .model
                .as_ref()
                .map(config_option::ModelRef::from)
                .unwrap_or_else(|| selected.model.clone());
            return Ok(SetSessionConfigOptionResponse {
                config_options: config_options(
                    &snapshot,
                    &ConfigState {
                        model,
                        variant: state.variant.as_deref(),
                        mode_id: state.mode_id.as_deref(),
                    },
                ),
            });
        }

        let model = current
            .model
            .as_ref()
            .map(directory::DefaultModel::from)
            .unwrap_or_else(|| select_default_model(&snapshot));

        if params.config_id == "effort" {
            let variants = directory::variants(&snapshot, &model);
            if variants.is_none_or(|variants| !variants.contains_key(value)) {
                return Err(ACPError::InvalidEffort {
                    effort: value.clone(),
                });
            }
            let state = self
                .session
                .set_variant(&params.session_id, Some(value.clone()))
                .await?;
            let model = state
                .model
                .as_ref()
                .map(config_option::ModelRef::from)
                .unwrap_or_else(|| config_option::ModelRef::from(&model));
            return Ok(SetSessionConfigOptionResponse {
                config_options: config_options(
                    &snapshot,
                    &ConfigState {
                        model,
                        variant: state.variant.as_deref(),
                        mode_id: state.mode_id.as_deref(),
                    },
                ),
            });
        }

        if params.config_id == "mode" {
            if !snapshot
                .available_modes
                .iter()
                .any(|mode| mode.id == *value)
            {
                return Err(ACPError::InvalidMode {
                    mode: value.clone(),
                });
            }
            let state = self
                .session
                .set_mode(&params.session_id, Some(value.clone()))
                .await?;
            let model = state
                .model
                .as_ref()
                .map(config_option::ModelRef::from)
                .unwrap_or_else(|| config_option::ModelRef::from(&select_default_model(&snapshot)));
            return Ok(SetSessionConfigOptionResponse {
                config_options: config_options(
                    &snapshot,
                    &ConfigState {
                        model,
                        variant: state.variant.as_deref(),
                        mode_id: state.mode_id.as_deref(),
                    },
                ),
            });
        }

        Err(ACPError::InvalidConfigOption {
            config_id: params.config_id.clone(),
        })
    }

    /// `setSessionMode` from reference/packages/opencode/src/acp/service.ts.
    pub async fn set_session_mode(
        &self,
        params: &SetSessionModeRequest,
    ) -> Result<SetSessionModeResponse, ACPError> {
        let current = self.session.get(&params.session_id).await?;
        let snapshot = self.config_snapshot(&current).await?;
        if !snapshot
            .available_modes
            .iter()
            .any(|mode| mode.id == params.mode_id)
        {
            return Err(ACPError::InvalidMode {
                mode: params.mode_id.clone(),
            });
        }
        self.session
            .set_mode(&params.session_id, Some(params.mode_id.clone()))
            .await?;
        Ok(SetSessionModeResponse::default())
    }

    /// `setSessionModel` from reference/packages/opencode/src/acp/service.ts.
    pub async fn set_session_model(
        &self,
        params: &SetSessionModelRequest,
    ) -> Result<SetSessionModelResponse, ACPError> {
        let current = self.session.get(&params.session_id).await?;
        let snapshot = self.config_snapshot(&current).await?;
        let selected = self
            .parse_selected_model(&snapshot, &params.model_id)
            .await?;
        let default_model = directory::DefaultModel::from(&selected.model);
        let variant = if directory::variants(&snapshot, &default_model).is_some() {
            selected
                .variant
                .clone()
                .or_else(|| select_variant(&snapshot, &default_model))
        } else {
            None
        };
        self.session
            .set_variant(&params.session_id, variant)
            .await?;
        self.session
            .set_model(
                &params.session_id,
                Some(session::SelectedModel::from(&selected.model)),
            )
            .await?;
        Ok(SetSessionModelResponse::default())
    }

    /// `prompt` from reference/packages/opencode/src/acp/service.ts.
    pub async fn prompt(&self, params: &PromptRequest) -> Result<PromptResponse, ACPError> {
        let current = self.session.get(&params.session_id).await?;
        let snapshot = self.directory_snapshot(&current.cwd).await?;
        let selected = current
            .model
            .as_ref()
            .map(directory::DefaultModel::from)
            .unwrap_or_else(|| select_default_model(&snapshot));
        if current.model.is_none() {
            self.session
                .set_model(
                    &params.session_id,
                    Some(session::SelectedModel::from(&selected)),
                )
                .await?;
        }
        let variant = current
            .variant
            .clone()
            .or_else(|| select_variant(&snapshot, &selected));
        let mode_id = current.mode_id.clone().or_else(|| mode_id_for(&snapshot));
        let parts = content::prompt_content_to_parts(&params.prompt);
        let command = detect_slash_command(&parts);

        let Some(command) = command else {
            let response = self
                .request(
                    self.sdk.session_prompt(SdkPromptRequest {
                        session_id: current.id.clone(),
                        model: crate::sdk::ModelSelection {
                            provider_id: selected.provider_id.clone(),
                            model_id: selected.model_id.clone(),
                        },
                        variant: variant.clone(),
                        parts,
                        agent: mode_id.clone(),
                        directory: current.cwd.clone(),
                    }),
                    Some("session"),
                )
                .await?;
            self.send_usage_update(&current.id, &current.cwd).await;
            return prompt_response(Some(&response), params.message_id.as_deref());
        };

        if let Some(known) = snapshot
            .available_commands
            .iter()
            .find(|item| item.name == command.name)
        {
            let response = self
                .request(
                    self.sdk.session_command(SdkCommandRequest {
                        session_id: current.id.clone(),
                        command: known.name.clone(),
                        arguments: command.args.clone(),
                        model: format!("{}/{}", selected.provider_id, selected.model_id),
                        variant: variant.clone(),
                        agent: mode_id.clone(),
                        directory: current.cwd.clone(),
                    }),
                    Some("session"),
                )
                .await?;
            self.send_usage_update(&current.id, &current.cwd).await;
            return prompt_response(Some(&response), params.message_id.as_deref());
        }

        if command.name == "compact" {
            self.request(
                self.sdk.session_summarize(SummarizeRequest {
                    session_id: current.id.clone(),
                    directory: current.cwd.clone(),
                    provider_id: selected.provider_id.clone(),
                    model_id: selected.model_id.clone(),
                }),
                Some("session"),
            )
            .await?;
        }

        self.send_usage_update(&current.id, &current.cwd).await;
        prompt_response(None, params.message_id.as_deref())
    }

    /// Wraps an SDK call, converting raw SDK errors via `fromUnknownError`.
    async fn request<T>(
        &self,
        future: impl Future<Output = Result<T, Value>>,
        service: Option<&str>,
    ) -> Result<T, ACPError> {
        match future.await {
            Ok(value) => Ok(value),
            Err(error) => Err(from_unknown_error(&error, service)),
        }
    }

    /// `profiledRequest` from reference/packages/opencode/src/acp/service.ts.
    async fn profiled_request<T>(
        &self,
        name: &str,
        future: impl Future<Output = Result<T, Value>>,
        service: Option<&str>,
    ) -> Result<T, ACPError> {
        let started = std::time::Instant::now();
        let result = self.request(future, service).await;
        crate::profile::duration(name, started, &[]);
        result
    }

    async fn send_usage_update(&self, session_id: &str, directory: &str) {
        let Some(connection) = self.connection.clone() else {
            return;
        };
        self.usage
            .send_update(connection.as_ref(), session_id, directory)
            .await;
    }

    fn send_available_commands(&self, session_id: &str, snapshot: &directory::Snapshot) {
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let session_id = session_id.to_string();
        let available_commands: Vec<AvailableCommand> = snapshot
            .available_commands
            .iter()
            .map(|command| AvailableCommand {
                name: command.name.clone(),
                description: command.description.clone().unwrap_or_default(),
            })
            .collect();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::ZERO).await;
            let _ = connection
                .session_update(
                    &session_id,
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate {
                        available_commands,
                    }),
                )
                .await;
        });
    }

    async fn register_mcp_servers(&self, directory: &str, session_id: &str, servers: &[McpServer]) {
        let started = std::time::Instant::now();
        let current = {
            let mut registered = self.registered_mcp.lock().await;
            registered
                .entry(session_id.to_string())
                .or_default()
                .clone()
        };
        let mut pending: HashSet<String> = HashSet::new();
        let mut jobs = Vec::new();
        for server in servers {
            let config = mcp_config(server);
            let key = mcp_registration_key(server.name(), &config);
            if current.contains(&key) || pending.contains(&key) {
                continue;
            }
            pending.insert(key.clone());
            let sdk = self.sdk.clone();
            let registered = self.registered_mcp.clone();
            let directory = directory.to_string();
            let session_id = session_id.to_string();
            let name = server.name().to_string();
            jobs.push(async move {
                let result = sdk.mcp_add(&directory, &name, config).await;
                if result.is_ok() {
                    registered
                        .lock()
                        .await
                        .entry(session_id)
                        .or_default()
                        .insert(key);
                }
            });
        }
        futures::future::join_all(jobs).await;
        crate::profile::duration(
            "acp.mcp.register",
            started,
            &[(
                "count",
                crate::profile::ProfileValue::Num(pending.len() as i64),
            )],
        );
    }

    async fn replay_messages(&self, messages: &[SessionMessageResponse]) {
        let Some(events) = &self.events else {
            return;
        };
        for message in messages {
            events.replay_message(message).await;
        }
    }

    async fn parse_selected_model(
        &self,
        snapshot: &directory::Snapshot,
        model_id: &str,
    ) -> Result<config_option::ModelSelection, ACPError> {
        let providers: Vec<ProviderInfo> = snapshot.providers.values().cloned().collect();
        let config_providers = config_option::providers_from_info(&providers);
        let selected = config_option::parse_model_selection(model_id, &config_providers);
        let provider = snapshot.providers.get(&selected.model.provider_id);
        let model = provider.and_then(|provider| provider.models.get(&selected.model.model_id));
        let Some(model) = model else {
            return Err(ACPError::InvalidModel {
                provider_id: Some(selected.model.provider_id.clone()),
                model_id: model_id.to_string(),
            });
        };
        if let Some(variant) = &selected.variant {
            let known = model
                .variants
                .as_ref()
                .is_some_and(|variants| variants.contains_key(variant));
            if !known {
                return Err(ACPError::InvalidEffort {
                    effort: variant.clone(),
                });
            }
        }
        let provider = provider.unwrap();
        Ok(config_option::ModelSelection {
            model: config_option::ModelRef {
                provider_id: provider.id.clone(),
                model_id: model.id.clone(),
            },
            variant: selected.variant,
        })
    }
}

/// `ConfigState` from reference/packages/opencode/src/acp/service.ts.
struct ConfigState<'a> {
    model: config_option::ModelRef,
    variant: Option<&'a str>,
    mode_id: Option<&'a str>,
}

impl From<&config_option::ModelRef> for directory::DefaultModel {
    fn from(model: &config_option::ModelRef) -> Self {
        directory::DefaultModel {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        }
    }
}

impl From<&config_option::ModelRef> for session::SelectedModel {
    fn from(model: &config_option::ModelRef) -> Self {
        session::SelectedModel {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        }
    }
}

impl From<&directory::DefaultModel> for config_option::ModelRef {
    fn from(model: &directory::DefaultModel) -> Self {
        config_option::ModelRef {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        }
    }
}

impl From<&directory::DefaultModel> for session::SelectedModel {
    fn from(model: &directory::DefaultModel) -> Self {
        session::SelectedModel {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        }
    }
}

impl From<&session::SelectedModel> for config_option::ModelRef {
    fn from(model: &session::SelectedModel) -> Self {
        config_option::ModelRef {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        }
    }
}

impl From<&session::SelectedModel> for directory::DefaultModel {
    fn from(model: &session::SelectedModel) -> Self {
        directory::DefaultModel {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        }
    }
}

/// `configOptions` from reference/packages/opencode/src/acp/service.ts.
fn config_options(snapshot: &directory::Snapshot, state: &ConfigState) -> Vec<SessionConfigOption> {
    let providers: Vec<ProviderInfo> = snapshot.providers.values().cloned().collect();
    let config_providers = config_option::providers_from_info(&providers);
    let modes: Vec<config_option::ConfigOptionMode> = snapshot
        .available_modes
        .iter()
        .map(|mode| config_option::ConfigOptionMode {
            id: mode.id.clone(),
            name: mode.name.clone(),
            description: mode.description.clone(),
        })
        .collect();
    config_option::build_config_options(config_option::BuildConfigOptionsInput {
        providers: &config_providers,
        current_model: &state.model,
        current_variant: state.variant,
        include_model_variants: None,
        modes: Some(&modes),
        current_mode_id: state.mode_id,
    })
}

/// `modeIdFor` — `snapshot.availableModes.length > 0 ? snapshot.defaultModeID : undefined`.
fn mode_id_for(snapshot: &directory::Snapshot) -> Option<String> {
    if snapshot.available_modes.is_empty() {
        None
    } else {
        Some(snapshot.default_mode_id.clone())
    }
}

/// `selectDefaultModel` from reference/packages/opencode/src/acp/service.ts.
fn select_default_model(snapshot: &directory::Snapshot) -> directory::DefaultModel {
    if let Some(default) = &snapshot.default_model {
        return default.clone();
    }
    if let Some(model) = snapshot.model_options.first() {
        return directory::DefaultModel {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        };
    }
    directory::DefaultModel {
        provider_id: "unknown".into(),
        model_id: "unknown".into(),
    }
}

/// `selectVariant` from reference/packages/opencode/src/acp/service.ts.
fn select_variant(
    snapshot: &directory::Snapshot,
    model: &directory::DefaultModel,
) -> Option<String> {
    let variants = directory::variants(snapshot, model)?;
    if variants.contains_key("default") {
        return Some("default".into());
    }
    variants.keys().next().cloned()
}

/// `detectSlashCommand` from reference/packages/opencode/src/acp/service.ts.
fn detect_slash_command(parts: &[PromptPart]) -> Option<SlashCommand> {
    let text: String = parts
        .iter()
        .filter_map(|part| match part {
            PromptPart::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>()
        .trim()
        .to_string();
    if !text.starts_with('/') {
        return None;
    }
    let mut parts = text[1..].split_whitespace();
    let name = parts.next()?;
    if name.is_empty() {
        return None;
    }
    let args = parts.collect::<Vec<_>>().join(" ").trim().to_string();
    Some(SlashCommand {
        name: name.to_string(),
        args,
    })
}

/// A detected slash command.
struct SlashCommand {
    name: String,
    args: String,
}

/// `promptResponse` from reference/packages/opencode/src/acp/service.ts.
fn prompt_response(
    info: Option<&AssistantMessage>,
    message_id: Option<&str>,
) -> Result<PromptResponse, ACPError> {
    let user_message_id = message_id.map(str::to_string);
    let usage = info.map(usage::build_usage);

    let Some(info) = info else {
        return Ok(PromptResponse {
            stop_reason: StopReason::EndTurn,
            usage: None,
            user_message_id,
            _meta: Map::new(),
        });
    };
    let Some(error) = &info.error else {
        return Ok(PromptResponse {
            stop_reason: StopReason::EndTurn,
            usage,
            user_message_id,
            _meta: Map::new(),
        });
    };

    let base = PromptResponse {
        stop_reason: StopReason::EndTurn,
        usage,
        user_message_id,
        _meta: Map::new(),
    };
    let name = error.get("name").and_then(Value::as_str);
    match name {
        Some("MessageAbortedError") => Ok(PromptResponse {
            stop_reason: StopReason::Cancelled,
            ..base
        }),
        Some("MessageOutputLengthError") => Ok(PromptResponse {
            stop_reason: StopReason::MaxTokens,
            ..base
        }),
        Some("ContentFilterError") => Ok(PromptResponse {
            stop_reason: StopReason::Refusal,
            ..base
        }),
        Some("ProviderAuthError") => {
            let provider_id = error
                .get("data")
                .and_then(|data| data.get("providerID"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Err(ACPError::AuthRequired { provider_id })
        }
        _ => {
            let safe_message = error
                .get("data")
                .and_then(|data| data.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("OpenCode prompt failed")
                .to_string();
            Err(ACPError::ServiceFailure {
                safe_message,
                service: Some("session".into()),
                error_name: name.map(str::to_string),
            })
        }
    }
}

/// `defaultModelFromConfig` from reference/packages/opencode/src/acp/service.ts.
fn default_model_from_config(
    configured_model: Option<&str>,
    providers: &IndexMap<String, ProviderInfo>,
) -> Option<directory::DefaultModel> {
    let configured = configured_model.map(parse_model);
    if let Some(configured) = &configured {
        if let Some(provider) = providers.get(&configured.provider_id) {
            if provider.models.contains_key(&configured.model_id) {
                return Some(configured.clone());
            }
        }
    }

    if let Some(opencode_provider) = providers.get("opencode") {
        let mut models: Vec<&ModelInfo> = opencode_provider.models.values().collect();
        directory::provider_sort(&mut models, |model| &model.id);
        if let Some(model) = models.first() {
            return Some(directory::DefaultModel {
                provider_id: opencode_provider.id.clone(),
                model_id: model.id.clone(),
            });
        }
    }

    let mut best: Vec<&ModelInfo> = providers
        .values()
        .flat_map(|provider| provider.models.values())
        .collect();
    directory::provider_sort(&mut best, |model| &model.id);
    if let Some(model) = best.first() {
        return Some(directory::DefaultModel {
            provider_id: model.provider_id.clone(),
            model_id: model.id.clone(),
        });
    }

    configured
}

/// `Provider.parseModel` from reference/packages/opencode/src/provider/provider.ts.
fn parse_model(model: &str) -> directory::DefaultModel {
    match model.find('/') {
        Some(separator) => directory::DefaultModel {
            provider_id: model[..separator].to_string(),
            model_id: model[separator + 1..].to_string(),
        },
        None => directory::DefaultModel {
            provider_id: model.to_string(),
            model_id: String::new(),
        },
    }
}

/// `restoreFromMessages` from reference/packages/opencode/src/acp/service.ts.
fn restore_from_messages(messages: &[SessionMessageResponse]) -> Restored {
    let user = messages.iter().rev().find(|message| match &message.info {
        Message::User(user) => user
            .model
            .as_ref()
            .is_some_and(|model| !model.provider_id.is_empty() && !model.model_id.is_empty()),
        _ => false,
    });
    if let Some(user) = user {
        if let Message::User(user) = &user.info {
            if let Some(model) = &user.model {
                return Restored {
                    model: Some(session::SelectedModel {
                        provider_id: model.provider_id.clone(),
                        model_id: model.model_id.clone(),
                    }),
                    variant: model.variant.clone(),
                    mode_id: user.agent.clone(),
                };
            }
        }
    }

    let assistant = messages.iter().rev().find(|message| match &message.info {
        Message::Assistant(assistant) => {
            !assistant.provider_id.is_empty() && !assistant.model_id.is_empty()
        }
        _ => false,
    });
    if let Some(assistant) = assistant {
        if let Message::Assistant(assistant) = &assistant.info {
            return Restored {
                model: Some(session::SelectedModel {
                    provider_id: assistant.provider_id.clone(),
                    model_id: assistant.model_id.clone(),
                }),
                variant: assistant.variant.clone(),
                mode_id: assistant.mode.clone().or_else(|| assistant.agent.clone()),
            };
        }
    }

    Restored {
        model: None,
        variant: None,
        mode_id: None,
    }
}

/// Result of [`restore_from_messages`].
struct Restored {
    model: Option<session::SelectedModel>,
    variant: Option<String>,
    mode_id: Option<String>,
}

/// `mcpConfig` from reference/packages/opencode/src/acp/service.ts.
fn mcp_config(server: &McpServer) -> Value {
    match server {
        McpServer::Http(server) => json!({
            "type": "remote",
            "url": server.url,
            "headers": headers_map(&server.headers),
        }),
        McpServer::Sse(server) => json!({
            "type": "remote",
            "url": server.url,
            "headers": headers_map(&server.headers),
        }),
        McpServer::Stdio(server) => {
            let mut command = Vec::with_capacity(1 + server.args.len());
            command.push(Value::String(server.command.clone()));
            command.extend(server.args.iter().map(|arg| Value::String(arg.clone())));
            json!({
                "type": "local",
                "command": command,
                "environment": env_map(&server.env),
            })
        }
    }
}

fn headers_map(headers: &[crate::types::HttpHeader]) -> Map<String, Value> {
    headers
        .iter()
        .map(|header| (header.name.clone(), Value::String(header.value.clone())))
        .collect()
}

fn env_map(env: &[crate::types::EnvVariable]) -> Map<String, Value> {
    env.iter()
        .map(|entry| (entry.name.clone(), Value::String(entry.value.clone())))
        .collect()
}

/// `mcpRegistrationKey` from reference/packages/opencode/src/acp/service.ts.
fn mcp_registration_key(name: &str, config: &Value) -> String {
    format!("{name}:{}", stable_stringify(config))
}

/// `stableStringify` from reference/packages/opencode/src/acp/service.ts.
fn stable_stringify(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(stable_stringify).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        stable_stringify(&map[*key])
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        _ => serde_json::to_string(value).unwrap(),
    }
}

/// `fromUnknownError` from reference/packages/opencode/src/acp/service.ts.
fn from_unknown_error(error: &Value, service: Option<&str>) -> ACPError {
    if let Some(acp) = acp_error_from_value(error) {
        return acp;
    }
    if is_auth_required(error) {
        return ACPError::AuthRequired {
            provider_id: find_provider_id(error),
        };
    }
    ACPError::ServiceFailure {
        safe_message: "OpenCode service failure".into(),
        service: service.map(str::to_string),
        error_name: None,
    }
}

/// `isACPError` + reconstruction from reference/packages/opencode/src/acp/service.ts.
fn acp_error_from_value(value: &Value) -> Option<ACPError> {
    let tag = value.get("_tag")?.as_str()?;
    if !tag.starts_with("ACP") {
        return None;
    }
    let string = |key: &str| value.get(key)?.as_str().map(str::to_string);
    match tag {
        "ACPSessionNotFoundError" => Some(ACPError::SessionNotFound {
            session_id: string("sessionId")?,
        }),
        "ACPInvalidConfigOptionError" => Some(ACPError::InvalidConfigOption {
            config_id: string("configId")?,
        }),
        "ACPInvalidModelError" => Some(ACPError::InvalidModel {
            model_id: string("modelId")?,
            provider_id: string("providerId"),
        }),
        "ACPInvalidEffortError" => Some(ACPError::InvalidEffort {
            effort: string("effort")?,
        }),
        "ACPInvalidModeError" => Some(ACPError::InvalidMode {
            mode: string("mode")?,
        }),
        "ACPAuthRequiredError" => Some(ACPError::AuthRequired {
            provider_id: string("providerId"),
        }),
        "ACPUnknownAuthMethodError" => Some(ACPError::UnknownAuthMethod {
            method_id: string("methodId")?,
        }),
        "ACPUnsupportedOperationError" => Some(ACPError::UnsupportedOperation {
            method: string("method")?,
        }),
        "ACPServiceFailureError" => Some(ACPError::ServiceFailure {
            safe_message: string("safeMessage").unwrap_or_default(),
            service: string("service"),
            error_name: string("errorName"),
        }),
        _ => None,
    }
}

/// `isAuthRequired` from reference/packages/opencode/src/acp/service.ts.
fn is_auth_required(value: &Value) -> bool {
    if let Some(name) = value.get("name").and_then(Value::as_str) {
        if name == "ProviderAuthError" || name == "LoadAPIKeyError" {
            return true;
        }
    }
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        if message.contains("ProviderAuthError") || message.contains("LoadAPIKeyError") {
            return true;
        }
    }
    if let Some(tag) = value.get("_tag").and_then(Value::as_str) {
        if tag == "ProviderAuthError" || tag == "LoadAPIKeyError" {
            return true;
        }
    }
    if let Some(error) = value.get("error") {
        if is_auth_required(error) {
            return true;
        }
    }
    if let Some(data) = value.get("data") {
        if is_auth_required(data) {
            return true;
        }
    }
    false
}

/// `findProviderID` from reference/packages/opencode/src/acp/service.ts.
fn find_provider_id(value: &Value) -> Option<String> {
    if let Some(id) = value.get("providerID").and_then(Value::as_str) {
        return Some(id.to_string());
    }
    if let Some(id) = value.get("providerId").and_then(Value::as_str) {
        return Some(id.to_string());
    }
    if let Some(data) = value.get("data") {
        return find_provider_id(data);
    }
    if let Some(error) = value.get("error") {
        return find_provider_id(error);
    }
    None
}

/// `loadDirectorySnapshot` from reference/packages/opencode/src/acp/service.ts.
async fn load_directory_snapshot(
    sdk: &dyn OpencodeClient,
    directory: &str,
) -> Result<directory::Snapshot, ACPError> {
    let started = std::time::Instant::now();
    let (providers_result, agents_result, commands_result, skills_result, config_result) = tokio::join!(
        sdk.config_providers(directory),
        sdk.app_agents(directory),
        sdk.command_list(directory),
        sdk.app_skills(directory),
        sdk.config_get(directory),
    );
    let providers_data =
        providers_result.map_err(|error| from_unknown_error(&error, Some("directory")))?;
    let agents = agents_result.map_err(|error| from_unknown_error(&error, Some("directory")))?;
    let commands_data =
        commands_result.map_err(|error| from_unknown_error(&error, Some("directory")))?;
    let skills = skills_result.map_err(|error| from_unknown_error(&error, Some("directory")))?;
    let config = config_result.ok();
    crate::profile::duration("acp.directory.load", started, &[]);

    let providers: IndexMap<String, ProviderInfo> = providers_data
        .providers
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect();
    let default_model = default_model_from_config(
        config.as_ref().and_then(|config| config.model.as_deref()),
        &providers,
    );
    let modes: Vec<directory::ModeOption> = agents
        .iter()
        .filter(|agent| agent.mode != "subagent" && agent.hidden != Some(true))
        .map(|agent| directory::ModeOption {
            id: agent.name.clone(),
            name: agent.name.clone(),
            description: agent.description.clone(),
        })
        .collect();
    let mut commands: Vec<CommandInfo> = commands_data;
    for skill in &skills {
        if !commands.iter().any(|command| command.name == skill.name) {
            commands.push(CommandInfo {
                name: skill.name.clone(),
                description: Some(skill.description.clone()),
                source: Some("skill".into()),
                template: Value::String(skill.content.clone()),
                hints: Vec::new(),
            });
        }
    }
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    let default_mode_id = agents
        .iter()
        .find(|agent| agent.mode == "primary" && agent.hidden != Some(true))
        .map(|agent| agent.name.clone())
        .unwrap_or_else(|| "build".into());

    Ok(directory::build(directory::BuildInput {
        directory: directory.to_string(),
        providers,
        modes,
        default_mode_id,
        commands,
        default_model,
    }))
}

struct SdkDirectoryLoader {
    sdk: Arc<dyn OpencodeClient>,
}

#[async_trait::async_trait]
impl directory::Loader for SdkDirectoryLoader {
    async fn load(&self, directory: &str) -> Result<directory::Snapshot, ACPError> {
        load_directory_snapshot(self.sdk.as_ref(), directory).await
    }
}

/// `new Date(ms).toISOString()`.
fn to_iso(ms: &i64) -> String {
    DateTime::from_timestamp_millis(*ms)
        .unwrap_or(DateTime::<Utc>::from_timestamp_millis(0).unwrap())
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// `new Date(updatedAt ?? 0).getTime()`.
fn entry_timestamp(info: &SessionInfo) -> f64 {
    info.updated_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis() as f64)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_handles_provider_and_model() {
        assert_eq!(
            parse_model("anthropic/claude-sonnet-4"),
            directory::DefaultModel {
                provider_id: "anthropic".into(),
                model_id: "claude-sonnet-4".into(),
            }
        );
        assert_eq!(
            parse_model("openai"),
            directory::DefaultModel {
                provider_id: "openai".into(),
                model_id: String::new(),
            }
        );
    }

    #[test]
    fn detect_slash_command_variants() {
        let parts = vec![PromptPart::Text {
            text: "  /compact now ".into(),
            synthetic: None,
            ignored: None,
        }];
        let command = detect_slash_command(&parts).unwrap();
        assert_eq!(command.name, "compact");
        assert_eq!(command.args, "now");
        let plain = vec![PromptPart::Text {
            text: "hello".into(),
            synthetic: None,
            ignored: None,
        }];
        assert!(detect_slash_command(&plain).is_none());
    }

    #[test]
    fn mcp_config_remote_and_local() {
        let http = McpServer::Http(crate::types::McpServerHttp {
            name: "remote".into(),
            url: "https://example.com".into(),
            headers: vec![crate::types::HttpHeader {
                name: "X-Key".into(),
                value: "v".into(),
            }],
        });
        assert_eq!(
            mcp_config(&http),
            json!({
                "type": "remote",
                "url": "https://example.com",
                "headers": { "X-Key": "v" }
            })
        );
        let stdio = McpServer::Stdio(crate::types::McpServerStdio {
            name: "local".into(),
            command: "npx".into(),
            args: vec!["-y".into()],
            env: vec![crate::types::EnvVariable {
                name: "FOO".into(),
                value: "bar".into(),
            }],
        });
        assert_eq!(
            mcp_config(&stdio),
            json!({
                "type": "local",
                "command": ["npx", "-y"],
                "environment": { "FOO": "bar" }
            })
        );
    }

    #[test]
    fn stable_stringify_sorts_keys() {
        let value = json!({ "b": 1, "a": [2, { "d": 3, "c": 4 }] });
        assert_eq!(stable_stringify(&value), r#"{"a":[2,{"c":4,"d":3}],"b":1}"#);
    }

    #[test]
    fn auth_required_detection() {
        assert!(is_auth_required(
            &json!({ "name": "ProviderAuthError", "data": { "providerID": "openai" } })
        ));
        assert!(is_auth_required(
            &json!({ "data": { "error": { "_tag": "LoadAPIKeyError" } } })
        ));
        assert!(!is_auth_required(&json!({ "name": "OtherError" })));
        assert_eq!(
            find_provider_id(&json!({ "data": { "error": { "providerID": "openai" } } })),
            Some("openai".into())
        );
    }

    #[test]
    fn from_unknown_error_maps() {
        assert!(matches!(
            from_unknown_error(
                &json!({ "name": "ProviderAuthError", "data": { "providerID": "x" } }),
                None
            ),
            ACPError::AuthRequired {
                provider_id: Some(_)
            }
        ));
        assert!(matches!(
            from_unknown_error(&json!({ "message": "boom" }), Some("session")),
            ACPError::ServiceFailure { service: Some(service), .. } if service == "session"
        ));
        assert!(matches!(
            from_unknown_error(&json!({ "_tag": "ACPSessionNotFoundError", "sessionId": "s" }), None),
            ACPError::SessionNotFound { session_id } if session_id == "s"
        ));
    }

    #[test]
    fn to_iso_matches_javascript() {
        assert_eq!(to_iso(&0), "1970-01-01T00:00:00.000Z");
        assert_eq!(to_iso(&1700000000123), "2023-11-14T22:13:20.123Z");
        assert_eq!(to_iso(&1785019008123), "2026-07-25T22:36:48.123Z");
    }

    #[test]
    fn default_model_falls_back_to_configured() {
        let mut providers = IndexMap::new();
        providers.insert(
            "opencode".into(),
            ProviderInfo {
                id: "opencode".into(),
                name: "opencode".into(),
                source: "env".into(),
                env: vec![],
                key: None,
                options: Map::new(),
                models: IndexMap::new(),
            },
        );
        let model = default_model_from_config(Some("opencode/gpt-5"), &providers);
        assert_eq!(
            model,
            Some(directory::DefaultModel {
                provider_id: "opencode".into(),
                model_id: "gpt-5".into(),
            })
        );
    }
}
