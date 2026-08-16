//! Shared application state backing the handlers.

use std::collections::{BTreeMap, HashMap, HashSet};
#[cfg(unix)]
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio_util::sync::CancellationToken;

use crate::auth::AuthConfig;
use crate::cors::CorsOptions;
use crate::event::EventBus;
use crate::location::Location;
use crate::schema::SessionInfo;
use indexmap::IndexMap;
use oc_database::tables::{
    json_columns, MessageRow, PartRow, PermissionRow, ProjectRow, SessionInputRow, SessionRow,
};
use oc_database::{Database, Value as SqlValue};

/// A stored PTY session. From reference/packages/schema/src/pty.ts (`Pty.Info`).
#[derive(Debug, Clone)]
pub struct PtyRecord {
    pub info: Value,
    pub running: bool,
    pub buffer: Vec<u8>,
    pub tickets: HashMap<String, i64>,
}

/// Live process handles for a PTY projection. The process map is deliberately
/// separate from `Stores` so the serializable API state stays lightweight.
pub(crate) enum PtyInput {
    #[cfg(unix)]
    Native(tokio::fs::File),
    #[cfg(not(unix))]
    Pipe(tokio::process::ChildStdin),
}

pub(crate) struct PtyProcess {
    pub child: Arc<Mutex<tokio::process::Child>>,
    pub stdin: Arc<Mutex<PtyInput>>,
    #[cfg(unix)]
    pub resize: Arc<OwnedFd>,
}

/// Last observed state of an MCP connection. The live client and native tool
/// definitions are held in the separate runtime maps on `AppState`; this
/// projection is the serializable/status-facing portion.
#[derive(Debug, Clone, Default)]
pub struct McpConnection {
    pub status: String,
    pub server_info: Option<Value>,
    pub tools: Vec<Value>,
    pub error: Option<String>,
}

/// A live MCP tool exposed to the session runner. The catalog key is the
/// OpenCode-compatible sanitized `server_tool` name, while `definition.name`
/// remains the native name sent over `tools/call`.
#[derive(Clone)]
pub struct McpRuntimeTool {
    pub server: String,
    pub definition: oc_mcp::types::Tool,
    pub client: Arc<oc_mcp::client::Client>,
    pub timeout: u64,
}

/// In-memory projection store.
#[derive(Debug, Default)]
pub struct Stores {
    pub sessions: HashMap<String, SessionRecord>,
    pub todos: HashMap<String, Value>,
    pub questions: HashMap<String, Value>,
    pub permissions: HashMap<String, Value>,
    /// Session-scoped "always allow" decisions. The reference keeps these
    /// in the permission-saved service; this projection is intentionally
    /// process-local until the corresponding durable table is added.
    pub saved_permissions: HashMap<String, oc_schema::permission_saved::Info>,
    pub pty: HashMap<String, PtyRecord>,
    pub config: Value,
}

impl Stores {
    pub fn new(config: Value) -> Self {
        Stores {
            config,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub info: SessionInfo,
    pub messages: Vec<Value>,
    pub active: bool,
}

/// An admitted prompt waiting for the session runner to promote it into the
/// next provider turn. The user message is persisted immediately; this small
/// process-local queue preserves the delivery mode while a turn is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSessionInput {
    pub id: String,
    pub seq: u64,
    pub delivery: String,
}

/// Process-local state for an integration OAuth attempt.
///
/// The provider-auth service owns the pending plugin callback, while the
/// server owns the public attempt ID, expiry, and status returned by the
/// integration routes. Keeping these concerns separate mirrors the reference
/// flow without putting transient authorization data in SQLite.
#[derive(Debug, Clone)]
pub(crate) struct IntegrationAttempt {
    pub provider_id: String,
    pub method: usize,
    pub attempt_id: String,
    pub url: String,
    pub instructions: String,
    pub mode: oc_provider::provider::auth::CallbackMethod,
    pub created: i64,
    pub expires: i64,
    pub status: IntegrationAttemptStatus,
}

#[derive(Debug, Clone)]
pub(crate) enum IntegrationAttemptStatus {
    Pending,
    Complete,
    Failed(String),
}

/// Global server state shared by all handlers via axum's `State` extractor.
#[derive(Clone)]
pub struct AppState {
    pub stores: Arc<RwLock<Stores>>,
    /// Process-local background jobs exposed by the experimental session
    /// background routes. Entries are intentionally not durable.
    pub background_jobs: Arc<oc_core::background_job::BackgroundJob>,
    pub events: EventBus,
    pub auth: Arc<AuthConfig>,
    pub cors: Arc<CorsOptions>,
    pub location: Arc<Location>,
    /// Host-owned provider auth hooks and pending authorization state. The
    /// default registry is empty until a host explicitly supplies hooks.
    pub provider_auth: Arc<oc_provider::provider::auth::BuiltinProviderAuth>,
    /// The production listener supplies a shared SQLite database. Tests and
    /// embedders can continue to use `AppState::new` for an in-memory-only
    /// projection.
    pub database: Option<Arc<Database>>,
    /// Durable session-event store used by the v1 sync/control-plane routes.
    /// The server event bus remains the low-latency projection stream; this
    /// store owns replay cursors and aggregate history.
    pub sync_store: Arc<oc_sync::sync::store::Store>,
    /// Local workspace/control-plane projection. Remote adapter transport is
    /// intentionally separate, but local lifecycle routes share this state.
    pub workspaces: Arc<Mutex<HashMap<String, Value>>>,
    pub mcp_connections: Arc<Mutex<HashMap<String, McpConnection>>>,
    pub mcp_clients: Arc<Mutex<HashMap<String, Arc<oc_mcp::client::Client>>>>,
    pub mcp_tools: Arc<Mutex<HashMap<String, McpRuntimeTool>>>,
    /// Shared MCP lifecycle service. Keeping this in application state makes
    /// pending OAuth registrations survive across the start/callback requests.
    pub mcp: Arc<oc_mcp::index::Mcp>,
    pub mcp_auth: Arc<oc_mcp::auth::McpAuth>,
    /// Optional production plugin runtime. The manager owns QuickJS plugins
    /// on a dedicated thread; tests and pure mode leave this unset.
    pub plugin_manager: Option<Arc<oc_plugin::PluginManager>>,
    pub plugin_reports: Arc<StdMutex<Vec<oc_plugin::PluginLoadReport>>>,
    /// Declarative registrations emitted by loaded plugins. The sink is
    /// shared with the QuickJS host so server handlers can consume
    /// command/skill/provider/agent registrations after bootstrap.
    pub plugin_registrations: Arc<oc_plugin::InMemoryRegistrationSink>,
    /// Short-lived integration OAuth attempts. Credentials themselves remain
    /// in the provider auth store; this map only tracks public attempt state.
    pub(crate) integration_attempts: Arc<Mutex<HashMap<String, IntegrationAttempt>>>,
    /// Process-backed language servers keyed by workspace and command.
    pub(crate) lsp_adapters: Arc<Mutex<HashMap<String, Arc<oc_project::lsp::LspAdapter>>>>,
    pub question_service: Arc<oc_command::question::QuestionService>,
    /// Project/worktree/snapshot services shared by session and project
    /// handlers. The reference boots these per instance; the server keeps a
    /// runtime manager and lets it memoize contexts by directory.
    pub project_runtime: Arc<oc_project::runtime::Runtime>,
    pub(crate) pty_processes: Arc<Mutex<HashMap<String, Arc<PtyProcess>>>>,
    permission_waiters: Arc<Mutex<HashMap<String, Arc<PermissionWaiter>>>>,
    pub(crate) session_runs: Arc<Mutex<HashMap<String, SessionRunState>>>,
    pub(crate) pending_inputs: Arc<Mutex<HashMap<String, Vec<PendingSessionInput>>>>,
}

/// Per-session run coordinator state. A second prompt arriving while a
/// provider turn is active marks the session for one follow-up drain instead
/// of starting a competing runner against the same history.
pub(crate) struct SessionRunState {
    pub rerun: bool,
    pub token: CancellationToken,
}

/// One in-flight permission prompt owned by a session-runner tool fiber.
/// The HTTP reply handler resolves it and wakes the suspended fiber.
pub(crate) struct PermissionWaiter {
    answer: Mutex<Option<bool>>,
    notify: Notify,
}

impl AppState {
    pub fn new(auth: AuthConfig, cors: CorsOptions, location: Location) -> Self {
        Self::new_with_config(auth, cors, location, default_config())
    }

    /// Construct state with a resolved ConfigV1.Info projection.
    ///
    /// The default constructor intentionally remains side-effect free for
    /// embedders and tests; the production listener uses this overload after
    /// resolving `opencode.json`/`opencode.jsonc`.
    pub fn new_with_config(
        auth: AuthConfig,
        cors: CorsOptions,
        location: Location,
        config: Value,
    ) -> Self {
        Self::new_with_config_and_provider_auth(auth, cors, location, config, BTreeMap::new())
    }

    /// Construct state with host-owned provider auth hooks. This is the
    /// bounded integration point for plugin hosts; no hooks means no claimed
    /// OAuth support and therefore no synthetic credentials.
    pub fn new_with_provider_auth(
        auth: AuthConfig,
        cors: CorsOptions,
        location: Location,
        hooks: BTreeMap<String, Box<dyn oc_provider::provider::auth::AuthHook>>,
    ) -> Self {
        Self::new_with_config_and_provider_auth(auth, cors, location, default_config(), hooks)
    }

    pub fn new_with_config_and_provider_auth(
        auth: AuthConfig,
        cors: CorsOptions,
        location: Location,
        config: Value,
        hooks: BTreeMap<String, Box<dyn oc_provider::provider::auth::AuthHook>>,
    ) -> Self {
        let mcp_auth = Arc::new(oc_mcp::auth::McpAuth::default());
        let mcp_config = config
            .get("mcp")
            .and_then(Value::as_object)
            .map(|servers| {
                servers
                    .iter()
                    .filter_map(|(name, value)| {
                        serde_json::from_value(value.clone())
                            .ok()
                            .map(|info| (name.clone(), info))
                    })
                    .collect::<IndexMap<String, oc_mcp::config::Info>>()
            })
            .unwrap_or_default();
        let mcp = oc_mcp::index::Mcp::with_options(
            mcp_config,
            PathBuf::from(&location.directory),
            oc_mcp::index::McpOptions {
                auth: Some(Arc::clone(&mcp_auth)),
                ..Default::default()
            },
        );
        let project_config = oc_project::util::config::Config {
            snapshot: config.get("snapshot").and_then(Value::as_bool),
            experimental_icon_discovery: config
                .get("experimental_icon_discovery")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        AppState {
            stores: Arc::new(RwLock::new(Stores::new(config))),
            background_jobs: Arc::new(oc_core::background_job::BackgroundJob::new()),
            events: EventBus::new(256),
            auth: Arc::new(auth),
            cors: Arc::new(cors),
            location: Arc::new(location),
            provider_auth: Arc::new(oc_provider::provider::auth::ProviderAuth::new(hooks)),
            database: None,
            sync_store: Arc::new(oc_sync::sync::store::Store::new()),
            workspaces: Arc::new(Mutex::new(HashMap::new())),
            mcp_connections: Arc::new(Mutex::new(HashMap::new())),
            mcp_clients: Arc::new(Mutex::new(HashMap::new())),
            mcp_tools: Arc::new(Mutex::new(HashMap::new())),
            mcp,
            mcp_auth,
            plugin_manager: None,
            plugin_reports: Arc::new(StdMutex::new(Vec::new())),
            plugin_registrations: Arc::new(oc_plugin::InMemoryRegistrationSink::default()),
            integration_attempts: Arc::new(Mutex::new(HashMap::new())),
            lsp_adapters: Arc::new(Mutex::new(HashMap::new())),
            question_service: Arc::new(oc_command::question::QuestionService::new()),
            project_runtime: oc_project::runtime::Runtime::new(project_config).into(),
            pty_processes: Arc::new(Mutex::new(HashMap::new())),
            permission_waiters: Arc::new(Mutex::new(HashMap::new())),
            session_runs: Arc::new(Mutex::new(HashMap::new())),
            pending_inputs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Construct the production state and hydrate its in-memory projection
    /// from the reference-compatible SQLite tables.
    pub fn with_database(
        auth: AuthConfig,
        cors: CorsOptions,
        location: Location,
        database: Arc<Database>,
    ) -> oc_database::Result<Self> {
        Self::with_database_and_config(auth, cors, location, database, default_config())
    }

    /// Construct production state from a database and the resolved config
    /// projection used by `/config`, command, skill, and provider handlers.
    pub fn with_database_and_config(
        auth: AuthConfig,
        cors: CorsOptions,
        location: Location,
        database: Arc<Database>,
        config: Value,
    ) -> oc_database::Result<Self> {
        let mut state = Self::new_with_config(auth, cors, location, config);
        state.sync_store = Arc::new(oc_sync::sync::store::Store::with_database(Arc::clone(
            &database,
        ))?);
        state.database = Some(database);
        state.load_persisted_sessions()?;
        Ok(state)
    }

    /// Publish an HTTP/SSE event and, when it is a registered durable session
    /// event, assign a monotonic sync cursor and retain it for replay. Keeping
    /// this fan-out at the state boundary prevents individual producers from
    /// silently bypassing the control-plane history store.
    pub fn emit_event(&self, mut event: crate::event::Event) {
        let version = event.durable.as_ref().map(|durable| durable.version);
        let definition = version
            .and_then(|version| {
                oc_sync::sync::store::durable_get(&format!("{}.{}", event.r#type, version))
            })
            .or_else(|| {
                (1..=2).find_map(|version| {
                    oc_sync::sync::store::durable_get(&format!("{}.{}", event.r#type, version))
                })
            });
        if let Some(definition) = definition {
            let location =
                event
                    .location
                    .as_ref()
                    .map(|location| oc_sync::sync::event::LocationRef {
                        directory: location.directory.clone(),
                        workspace_id: location.workspace_id.clone(),
                    });
            let result = self.sync_store.publish(
                &definition,
                event.data.clone(),
                oc_sync::sync::store::PublishOptions {
                    id: Some(oc_sync::sync::event::EventID::from(event.id.clone())),
                    metadata: event.metadata.clone().map(Value::Object),
                    location,
                    commit: None,
                },
            );
            match result {
                Ok(payload) => {
                    if let Some(durable) = payload.durable {
                        event.durable = Some(crate::event::Durable {
                            aggregate_id: durable.aggregate_id,
                            seq: durable.seq,
                            version: durable.version,
                        });
                    }
                }
                Err(error) => {
                    tracing::warn!(event_type = %event.r#type, ?error, "failed to retain durable sync event")
                }
            }
        }

        // Plugin event hooks are part of the server event fan-out, not merely
        // an isolated manager capability. Keep the synchronous state boundary
        // deterministic: a plugin failure is observable in logs but must not
        // prevent SSE delivery or durable projection publication.
        if let Some(manager) = &self.plugin_manager {
            let plugin_event = serde_json::to_value(&event).unwrap_or_else(|_| {
                serde_json::json!({
                    "id": event.id.clone(),
                    "type": event.r#type.clone(),
                    "data": event.data.clone(),
                })
            });
            if let Err(error) = manager.event(plugin_event) {
                tracing::warn!(event_type = %event.r#type, ?error, "plugin event hook failed");
            }
            if let Err(error) =
                manager.stream_event(serde_json::to_value(&event).unwrap_or_else(|_| {
                    serde_json::json!({
                        "id": event.id.clone(),
                        "type": event.r#type.clone(),
                        "data": event.data.clone(),
                    })
                }))
            {
                tracing::debug!(event_type = %event.r#type, ?error, "plugin SSE stream enqueue failed");
            }
        }
        self.events.emit(event);
    }

    fn load_persisted_sessions(&mut self) -> oc_database::Result<()> {
        let Some(database) = self.database.as_ref() else {
            return Ok(());
        };
        let rows = database.list_sessions(false)?;
        let saved_permissions = database.list::<PermissionRow>("permission", &[])?;
        let pending_inputs = database.list::<SessionInputRow>("session_input", &[])?;
        let mut sessions = HashMap::with_capacity(rows.len());
        for row in rows {
            let messages = load_messages(database, &row.id)?;
            let info = session_info_from_row(&row);
            sessions.insert(
                row.id,
                SessionRecord {
                    info,
                    messages,
                    active: false,
                },
            );
        }
        let stores =
            Arc::get_mut(&mut self.stores).expect("new state has the only stores reference");
        stores.get_mut().sessions = sessions;
        stores.get_mut().saved_permissions = saved_permissions
            .into_iter()
            .map(|row| {
                let info = oc_schema::permission_saved::Info {
                    id: row.id,
                    project_id: row.project_id,
                    action: row.action,
                    resource: row.resource,
                };
                (info.id.clone(), info)
            })
            .collect();
        let mut pending = self
            .pending_inputs
            .try_lock()
            .expect("new state has no concurrent pending-input readers");
        for row in pending_inputs {
            if row.promoted_seq.is_none() {
                pending
                    .entry(row.session_id)
                    .or_default()
                    .push(PendingSessionInput {
                        id: row.id,
                        seq: row.admitted_seq.max(0) as u64,
                        delivery: row.delivery,
                    });
            }
        }
        Ok(())
    }

    fn persist_saved_permission(&self, info: &oc_schema::permission_saved::Info) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        let now = timestamp();
        let row = PermissionRow {
            id: info.id.clone(),
            project_id: info.project_id.clone(),
            action: info.action.clone(),
            resource: info.resource.clone(),
            time_created: now,
            time_updated: now,
        };
        if let Err(error) = database.upsert(
            "permission",
            &row,
            json_columns("permission"),
            "id",
            &SqlValue::Text(row.id.clone()),
        ) {
            tracing::error!(permission_id = %info.id, ?error, "failed to persist saved permission");
        }
    }

    pub(crate) fn delete_saved_permission(&self, id: &str) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        if let Err(error) = database.delete_by("permission", "id", &SqlValue::Text(id.to_string()))
        {
            tracing::error!(
                permission_id = id,
                ?error,
                "failed to delete saved permission"
            );
        }
    }

    /// Persist a session projection. Errors are logged so a transient disk
    /// failure does not turn a successful provider response into a lost HTTP
    /// response; the next mutation retries the upsert.
    pub fn persist_session(&self, info: &SessionInfo) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        if let Err(error) = persist_session(database, info) {
            tracing::error!(session_id = %info.id, ?error, "failed to persist session");
        }
    }

    /// Persist a v1/v2 message projection.
    pub fn persist_message(&self, session_id: &str, message: &Value) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        let Some(message_id) = message.get("id").and_then(Value::as_str) else {
            tracing::warn!(session_id, "skipping message without an id");
            return;
        };
        let created = message_time(message, "created").unwrap_or_else(now_millis);
        let updated = message_time(message, "updated").unwrap_or(created);
        let row = MessageRow {
            id: message_id.to_string(),
            session_id: session_id.to_string(),
            time_created: created,
            time_updated: updated,
            data: message.clone(),
        };
        if let Err(error) = database.upsert(
            "message",
            &row,
            json_columns("message"),
            "id",
            &SqlValue::Text(row.id.clone()),
        ) {
            tracing::error!(session_id, message_id, ?error, "failed to persist message");
        }
    }

    /// Persist a message part when a runner produces one.
    pub fn persist_part(&self, session_id: &str, message_id: &str, part: &Value) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        let Some(part_id) = part.get("id").and_then(Value::as_str) else {
            tracing::warn!(session_id, message_id, "skipping part without an id");
            return;
        };
        let created = part_time(part, "start");
        let updated = part_time(part, "end");
        let row = PartRow {
            id: part_id.to_string(),
            message_id: message_id.to_string(),
            session_id: session_id.to_string(),
            time_created: created,
            time_updated: updated,
            data: part.clone(),
        };
        if let Err(error) = database.upsert(
            "part",
            &row,
            json_columns("part"),
            "id",
            &SqlValue::Text(row.id.clone()),
        ) {
            tracing::error!(
                session_id,
                message_id,
                part_id,
                ?error,
                "failed to persist part"
            );
        }
    }

    pub fn delete_session(&self, session_id: &str) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        if let Err(error) =
            database.delete_by("session", "id", &SqlValue::Text(session_id.to_string()))
        {
            tracing::error!(session_id, ?error, "failed to delete session");
        }
    }

    /// Delete a message and its parts from the durable projection.
    pub fn delete_message(&self, session_id: &str, message_id: &str) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        for (table, column, value) in [
            ("part", "message_id", message_id),
            ("message", "id", message_id),
        ] {
            if let Err(error) =
                database.delete_by(table, column, &SqlValue::Text(value.to_string()))
            {
                tracing::error!(
                    session_id,
                    message_id,
                    table,
                    ?error,
                    "failed to delete message projection"
                );
            }
        }
    }

    /// Delete one durable message part.
    pub fn delete_part(&self, session_id: &str, message_id: &str, part_id: &str) {
        let Some(database) = self.database.as_ref() else {
            return;
        };
        if let Err(error) = database.delete_by("part", "id", &SqlValue::Text(part_id.to_string())) {
            tracing::error!(
                session_id,
                message_id,
                part_id,
                ?error,
                "failed to delete part projection"
            );
        }
    }

    /// Ask the connected client for permission and suspend until its reply.
    /// Read-only workspace operations are admitted by the runner before this
    /// method; writes/processes use this durable HTTP-visible gate.
    pub(crate) async fn request_permission(
        &self,
        session_id: &str,
        permission: &str,
        patterns: Vec<String>,
        metadata: Value,
    ) -> bool {
        let id = crate::event::permission_id();
        let waiter = Arc::new(PermissionWaiter {
            answer: Mutex::new(None),
            notify: Notify::new(),
        });
        self.permission_waiters
            .lock()
            .await
            .insert(id.clone(), waiter.clone());
        let request = serde_json::json!({
            "id": id,
            "sessionID": session_id,
            "action": permission,
            "resources": patterns.clone(),
            "permission": permission,
            "patterns": patterns.clone(),
            "always": patterns.clone(),
            "metadata": metadata,
        });
        let v2_request = serde_json::json!({
            "id": id,
            "sessionID": session_id,
            "action": permission,
            "resources": patterns,
            "metadata": request.get("metadata").cloned().unwrap_or(Value::Null),
        });
        self.stores
            .write()
            .await
            .permissions
            .insert(id.clone(), request.clone());
        self.emit_event(crate::event::Event {
            id: crate::event::event_id(),
            metadata: None,
            r#type: "permission.asked".into(),
            durable: None,
            location: None,
            data: request,
        });
        self.emit_event(crate::event::Event {
            id: crate::event::event_id(),
            metadata: None,
            r#type: "permission.v2.asked".into(),
            durable: None,
            location: None,
            data: v2_request,
        });

        let allowed = tokio::time::timeout(std::time::Duration::from_secs(300), async {
            loop {
                if let Some(answer) = *waiter.answer.lock().await {
                    break answer;
                }
                waiter.notify.notified().await;
            }
        })
        .await
        .unwrap_or(false);
        self.permission_waiters.lock().await.remove(&id);
        self.stores.write().await.permissions.remove(&id);
        allowed
    }

    /// Resolve a live permission prompt. Returns false when no runner is
    /// waiting for the id; ordinary externally-created permission records can
    /// still be removed by their legacy handler.
    pub(crate) async fn resolve_permission(&self, request_id: &str, reply: &Value) -> bool {
        let allowed = permission_allowed(reply);
        let reply_value = reply.get("reply").cloned().unwrap_or_else(|| reply.clone());
        let mut waiters = self.permission_waiters.lock().await;
        let Some(waiter) = waiters.get(request_id).cloned() else {
            return false;
        };
        let request = self
            .stores
            .read()
            .await
            .permissions
            .get(request_id)
            .cloned()
            .unwrap_or(Value::Null);
        let session_id = request
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        // The reference rejects all other pending asks for this session when
        // one ask is rejected. Remove their waiters before notifying them so
        // a second reply cannot race and publish a duplicate approval.
        let mut resolved = vec![(request_id.to_string(), waiter, request.clone())];
        waiters.remove(request_id);
        if reply_value.as_str() == Some("reject") {
            let stores = self.stores.read().await;
            let pending_ids = stores
                .permissions
                .iter()
                .filter(|(id, value)| {
                    id.as_str() != request_id
                        && value
                            .get("sessionID")
                            .and_then(Value::as_str)
                            .is_some_and(|id| id == session_id)
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for pending_id in pending_ids {
                let Some(waiter) = waiters.remove(&pending_id) else {
                    continue;
                };
                let Some(request) = stores.permissions.get(&pending_id).cloned() else {
                    continue;
                };
                resolved.push((pending_id, waiter, request));
            }
        }
        if reply_value.as_str() == Some("reject") {
            let mut stores = self.stores.write().await;
            for (resolved_id, _, _) in &resolved {
                stores.permissions.remove(resolved_id);
            }
        }
        drop(waiters);

        for (_, waiter, _) in &resolved {
            *waiter.answer.lock().await = Some(allowed);
            waiter.notify.notify_waiters();
        }

        for (resolved_id, _, resolved_request) in &resolved {
            let resolved_session_id = resolved_request
                .get("sessionID")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let resolved_reply = if reply_value.as_str() == Some("reject") {
                Value::String("reject".into())
            } else {
                reply_value.clone()
            };
            self.emit_event(crate::event::Event {
                id: crate::event::event_id(),
                metadata: None,
                r#type: "permission.replied".into(),
                durable: None,
                location: None,
                data: serde_json::json!({
                    "sessionID": resolved_session_id,
                    "requestID": resolved_id,
                    "reply": resolved_reply.clone(),
                }),
            });
            self.emit_event(crate::event::Event {
                id: crate::event::event_id(),
                metadata: None,
                r#type: "permission.v2.replied".into(),
                durable: None,
                location: None,
                data: serde_json::json!({
                    "sessionID": resolved_session_id,
                    "requestID": resolved_id,
                    "reply": resolved_reply,
                }),
            });
        }
        if allowed && reply_value.as_str() == Some("always") {
            let permission = request
                .get("permission")
                .or_else(|| request.get("action"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let resources = request
                .get("patterns")
                .or_else(|| request.get("resources"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            let project_id = self
                .stores
                .read()
                .await
                .sessions
                .get(&session_id)
                .map(|session| session.info.project_id.clone())
                .unwrap_or_else(|| "global".to_string());
            let mut saved_permissions = Vec::with_capacity(resources.len());
            let mut stores = self.stores.write().await;
            for resource in resources {
                let saved = oc_schema::permission_saved::Info {
                    id: oc_schema::permission_saved::create_id(),
                    project_id: project_id.clone(),
                    action: permission.clone(),
                    resource,
                };
                stores
                    .saved_permissions
                    .insert(saved.id.clone(), saved.clone());
                saved_permissions.push(saved);
            }
            drop(stores);
            for saved in &saved_permissions {
                self.persist_saved_permission(saved);
            }
        }
        true
    }

    /// Acquire the single runner slot for a session. `None` means a runner is
    /// already active; the caller's prompt has been recorded and will be
    /// drained when that runner finishes.
    pub(crate) async fn acquire_session_run(&self, session_id: &str) -> Option<CancellationToken> {
        let mut runs = self.session_runs.lock().await;
        if let Some(run) = runs.get_mut(session_id) {
            run.rerun = true;
            return None;
        }
        let token = CancellationToken::new();
        runs.insert(
            session_id.to_string(),
            SessionRunState {
                rerun: false,
                token: token.clone(),
            },
        );
        Some(token)
    }

    /// Return the cancellation token for the currently running session, if
    /// one exists. Child-session adapters use this to inherit cancellation
    /// from the parent without coupling the tool crate to server state.
    pub(crate) async fn session_run_token(&self, session_id: &str) -> Option<CancellationToken> {
        self.session_runs
            .lock()
            .await
            .get(session_id)
            .map(|run| run.token.clone())
    }

    pub(crate) async fn enqueue_session_input(
        &self,
        session_id: &str,
        id: impl Into<String>,
        prompt: Value,
        seq: u64,
        delivery: impl Into<String>,
    ) {
        let id = id.into();
        let delivery = delivery.into();
        if let Some(database) = self.database.as_ref() {
            let row = SessionInputRow {
                id: id.clone(),
                session_id: session_id.to_string(),
                prompt,
                delivery: delivery.clone(),
                admitted_seq: seq.min(i64::MAX as u64) as i64,
                promoted_seq: None,
                time_created: now_millis() as i64,
            };
            if let Err(error) = database.upsert(
                "session_input",
                &row,
                json_columns("session_input"),
                "id",
                &SqlValue::Text(id.clone()),
            ) {
                tracing::warn!(session_id, %error, "failed to persist session input");
            }
        }
        self.pending_inputs
            .lock()
            .await
            .entry(session_id.to_string())
            .or_default()
            .push(PendingSessionInput { id, seq, delivery });
    }

    pub(crate) async fn pending_session_input(&self, session_id: &str, delivery: &str) -> bool {
        self.pending_inputs
            .lock()
            .await
            .get(session_id)
            .is_some_and(|inputs| inputs.iter().any(|input| input.delivery == delivery))
    }

    pub(crate) async fn promote_session_steers(&self, session_id: &str, cutoff: u64) -> u64 {
        let mut pending = self.pending_inputs.lock().await;
        let Some(inputs) = pending.get_mut(session_id) else {
            return 0;
        };
        let mut promoted_rows = Vec::new();
        inputs.retain(|input| {
            let promote = input.delivery == "steer" && input.seq <= cutoff;
            if promote {
                promoted_rows.push((input.id.clone(), input.seq));
            }
            !promote
        });
        let promoted = promoted_rows.len() as u64;
        if inputs.is_empty() {
            pending.remove(session_id);
        }
        drop(pending);
        self.mark_session_inputs_promoted(session_id, &promoted_rows);
        promoted
    }

    pub(crate) async fn promote_next_session_queue(&self, session_id: &str) -> bool {
        let mut pending = self.pending_inputs.lock().await;
        let Some(inputs) = pending.get_mut(session_id) else {
            return false;
        };
        let Some((index, _)) = inputs
            .iter()
            .enumerate()
            .filter(|(_, input)| input.delivery == "queue")
            .min_by_key(|(_, input)| input.seq)
        else {
            return false;
        };
        let input = inputs.remove(index);
        if inputs.is_empty() {
            pending.remove(session_id);
        }
        drop(pending);
        self.mark_session_inputs_promoted(session_id, &[(input.id, input.seq)]);
        true
    }

    fn mark_session_inputs_promoted(&self, session_id: &str, rows: &[(String, u64)]) {
        for (id, seq) in rows {
            self.emit_event(crate::event::Event {
                id: crate::event::event_id(),
                metadata: None,
                r#type: "session.input.promoted".into(),
                durable: None,
                location: None,
                data: serde_json::json!({
                    "sessionID": session_id,
                    "inputID": id,
                    "messageID": id,
                    "admittedSeq": seq,
                }),
            });
        }
        let Some(database) = self.database.as_ref() else {
            return;
        };
        for (id, seq) in rows {
            if let Err(error) = database.update_by(
                "session_input",
                "promoted_seq",
                &SqlValue::Integer((*seq).min(i64::MAX as u64) as i64),
                "id",
                &SqlValue::Text(id.clone()),
            ) {
                tracing::warn!(session_id, %error, "failed to persist promoted session input");
            }
        }
    }

    pub(crate) async fn latest_session_sequence(&self, session_id: &str) -> u64 {
        self.stores
            .read()
            .await
            .sessions
            .get(session_id)
            .map(|record| record.messages.len() as u64)
            .unwrap_or(0)
    }

    /// Interrupt an active turn for a steer prompt while retaining a rerun
    /// marker so the scheduler starts a fresh pass after cancellation drains.
    pub(crate) async fn interrupt_session_for_steer(&self, session_id: &str) {
        if let Some(run) = self.session_runs.lock().await.get_mut(session_id) {
            run.rerun = true;
            run.token.cancel();
        }
    }

    pub(crate) async fn pending_session_input_ids(&self, session_id: &str) -> HashSet<String> {
        self.pending_inputs
            .lock()
            .await
            .get(session_id)
            .into_iter()
            .flatten()
            .map(|input| input.id.clone())
            .collect()
    }

    /// Finish one runner pass. Returns a fresh token when a prompt arrived
    /// while the pass was active, otherwise releases the session slot.
    pub(crate) async fn finish_session_run(&self, session_id: &str) -> Option<CancellationToken> {
        let mut runs = self.session_runs.lock().await;
        let run = runs.get_mut(session_id)?;
        if run.rerun {
            run.rerun = false;
            let token = CancellationToken::new();
            run.token = token.clone();
            Some(token)
        } else {
            runs.remove(session_id);
            None
        }
    }

    /// Cooperatively stop the current provider/tool turn. A later prompt can
    /// still acquire the slot and start a new turn after cancellation drains.
    pub(crate) async fn cancel_session_run(&self, session_id: &str) {
        if let Some(run) = self.session_runs.lock().await.get_mut(session_id) {
            run.rerun = false;
            run.token.cancel();
        }
    }
}

fn permission_allowed(reply: &Value) -> bool {
    let value = reply
        .as_str()
        .or_else(|| reply.get("reply").and_then(Value::as_str))
        .or_else(|| reply.get("action").and_then(Value::as_str));
    matches!(
        value,
        Some("allow") | Some("approved") | Some("approve") | Some("once") | Some("always")
    ) || reply.as_bool() == Some(true)
}

fn persist_session(database: &Database, info: &SessionInfo) -> oc_database::Result<()> {
    let now = info.time.updated;
    let project = ProjectRow {
        id: info.project_id.clone(),
        worktree: info.location.directory.clone(),
        vcs: None,
        name: None,
        icon_url: None,
        icon_url_override: None,
        icon_color: None,
        time_created: info.time.created,
        time_updated: now,
        time_initialized: None,
        sandboxes: serde_json::json!([]),
        commands: None,
    };
    database.upsert(
        "project",
        &project,
        json_columns("project"),
        "id",
        &SqlValue::Text(project.id.clone()),
    )?;

    let row = SessionRow {
        id: info.id.clone(),
        project_id: info.project_id.clone(),
        workspace_id: info.location.workspace_id.clone(),
        parent_id: info.parent_id.clone(),
        slug: info
            .id
            .split('_')
            .next_back()
            .unwrap_or(&info.id)
            .to_string(),
        directory: info.location.directory.clone(),
        path: info.subpath.clone(),
        title: info.title.clone(),
        version: crate::version().to_string(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        metadata: None,
        cost: info.cost,
        tokens_input: info.tokens.input as i64,
        tokens_output: info.tokens.output as i64,
        tokens_reasoning: info.tokens.reasoning as i64,
        tokens_cache_read: info.tokens.cache.read as i64,
        tokens_cache_write: info.tokens.cache.write as i64,
        revert: info.revert.clone(),
        permission: None,
        agent: info.agent.clone(),
        model: info.model.as_ref().map(|model| serde_json::json!(model)),
        time_created: info.time.created,
        time_updated: info.time.updated,
        time_compacting: None,
        time_archived: info.time.archived,
    };
    database.upsert(
        "session",
        &row,
        json_columns("session"),
        "id",
        &SqlValue::Text(row.id.clone()),
    )
}

fn session_info_from_row(row: &SessionRow) -> SessionInfo {
    SessionInfo {
        id: row.id.clone(),
        parent_id: row.parent_id.clone(),
        project_id: row.project_id.clone(),
        agent: row.agent.clone(),
        model: row
            .model
            .as_ref()
            .and_then(|model| serde_json::from_value(model.clone()).ok()),
        cost: row.cost,
        tokens: crate::schema::Tokens {
            input: row.tokens_input as f64,
            output: row.tokens_output as f64,
            reasoning: row.tokens_reasoning as f64,
            cache: crate::schema::CacheTokens {
                read: row.tokens_cache_read as f64,
                write: row.tokens_cache_write as f64,
            },
        },
        time: crate::schema::SessionTime {
            created: row.time_created,
            updated: row.time_updated,
            archived: row.time_archived,
        },
        title: row.title.clone(),
        location: crate::schema::LocationRef {
            directory: row.directory.clone(),
            workspace_id: row.workspace_id.clone(),
        },
        subpath: row.path.clone(),
        revert: row.revert.clone(),
    }
}

fn load_messages(database: &Database, session_id: &str) -> oc_database::Result<Vec<Value>> {
    let page_size = 1_000_i64;
    let mut before: Option<(String, i64)> = None;
    let mut rows = Vec::new();
    loop {
        let cursor = before.as_ref().map(|(id, time)| (id.as_str(), *time));
        let page = database.list_messages_page(session_id, page_size, cursor)?;
        let has_more = page.len() > page_size as usize;
        let take = page
            .into_iter()
            .take(page_size as usize)
            .collect::<Vec<_>>();
        if let Some(oldest) = take.last() {
            before = Some((oldest.id.clone(), oldest.time_created));
        }
        rows.extend(take.into_iter().map(|row| row.data));
        if !has_more {
            break;
        }
    }
    rows.reverse();
    Ok(rows)
}

fn message_time(value: &Value, field: &str) -> Option<i64> {
    value
        .get("time")
        .and_then(|time| time.get(field))
        .and_then(Value::as_i64)
        .or_else(|| value.get("timeCreated").and_then(Value::as_i64))
}

fn part_time(value: &Value, field: &str) -> i64 {
    value
        .get("time")
        .and_then(|time| time.get(field))
        .and_then(Value::as_i64)
        .unwrap_or_else(now_millis)
}

/// Empty ConfigV1.Info shape. From reference/packages/core/src/v1/config/config.ts.
pub fn default_config() -> Value {
    serde_json::json!({})
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn timestamp() -> i64 {
    now_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct EventRecordingHost {
        logs: StdMutex<Vec<String>>,
    }

    impl oc_plugin::PluginHost for EventRecordingHost {
        fn log(&self, _level: &str, message: &str) {
            self.logs
                .lock()
                .expect("log lock")
                .push(message.to_string());
        }
    }

    #[test]
    fn state_bootstraps() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        assert_eq!(state.auth.username, "opencode");
        assert!(!state.auth.required());
    }

    #[test]
    fn durable_events_are_assigned_to_sync_history() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let mut events = state.events.subscribe();
        state.emit_event(crate::event::Event {
            id: "evt_sync_projection".into(),
            metadata: None,
            r#type: "session.updated".into(),
            durable: None,
            location: None,
            data: serde_json::json!({ "sessionID": "ses_sync_projection" }),
        });
        let event = events.try_recv().expect("event emitted");
        assert_eq!(event.durable.as_ref().map(|durable| durable.seq), Some(0));
        assert_eq!(state.sync_store.latest_sequence("ses_sync_projection"), 0);
        assert_eq!(
            state
                .sync_store
                .read_after("ses_sync_projection", -1)
                .expect("history"),
            vec![oc_sync::sync::event::Payload {
                id: "evt_sync_projection".into(),
                metadata: None,
                r#type: "session.updated".into(),
                durable: Some(oc_sync::sync::event::DurableEnvelope {
                    aggregate_id: "ses_sync_projection".into(),
                    seq: 0,
                    version: 1,
                }),
                location: None,
                data: serde_json::json!({ "sessionID": "ses_sync_projection" }),
            }]
        );
    }

    #[test]
    fn server_event_fanout_invokes_loaded_plugin_event_hooks() {
        let host = Arc::new(EventRecordingHost::default());
        let manager = Arc::new(oc_plugin::PluginManager::with_host(host.clone()));
        let spec = format!(
            "file://{}/tests/fixtures/event.ts",
            env!("CARGO_MANIFEST_DIR").replace("oc-server", "oc-plugin")
        );
        let report = manager.load_local(spec, serde_json::json!({}), None);
        assert!(report.error.is_none(), "plugin failed to load: {report:?}");

        let mut state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        state.plugin_manager = Some(manager);
        state.emit_event(crate::event::Event {
            id: "evt_plugin_hook".into(),
            metadata: None,
            r#type: "session.updated".into(),
            durable: None,
            location: None,
            data: serde_json::json!({}),
        });

        assert_eq!(
            host.logs.lock().expect("log lock").as_slice(),
            ["received:session.updated"]
        );
    }

    #[test]
    fn server_event_fanout_delivers_plugin_sse_streams() {
        let host = Arc::new(EventRecordingHost::default());
        let manager = Arc::new(oc_plugin::PluginManager::with_host(host.clone()));
        let spec = format!(
            "file://{}/tests/fixtures/stream.ts",
            env!("CARGO_MANIFEST_DIR").replace("oc-server", "oc-plugin")
        );
        let report = manager.load_local(spec, serde_json::json!({}), None);
        assert!(report.error.is_none(), "plugin failed to load: {report:?}");

        let mut state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        state.plugin_manager = Some(manager.clone());
        state.emit_event(crate::event::Event {
            id: "evt_plugin_stream".into(),
            metadata: None,
            r#type: "session.updated".into(),
            durable: None,
            location: None,
            data: serde_json::json!({}),
        });

        for _ in 0..100 {
            if host.logs.lock().expect("log lock").as_slice() == ["stream:session.updated"] {
                manager.dispose().expect("dispose after stream failed");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        manager.dispose().expect("dispose after stream failed");
        panic!("server event was not delivered to plugin SSE stream");
    }

    #[tokio::test]
    async fn state_keeps_resolved_config_projection() {
        let config = serde_json::json!({
            "model": "openai/gpt-5",
            "command": {"review": {"template": "Review the diff"}}
        });
        let state = AppState::new_with_config(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            config.clone(),
        );

        assert_eq!(state.stores.read().await.config, config);
    }

    #[tokio::test]
    async fn durable_state_round_trips_session_message_and_pending_input() {
        let database = Arc::new(Database::open_memory().expect("database"));
        let state = AppState::with_database(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode", None),
            database.clone(),
        )
        .expect("state");
        let info = SessionInfo {
            id: "ses_roundtrip".into(),
            parent_id: None,
            project_id: "prj_roundtrip".into(),
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
                updated: 2,
                archived: None,
            },
            title: "Round trip".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        };
        state.persist_session(&info);
        state.persist_message(
            &info.id,
            &crate::schema::message::user("msg_roundtrip", 3, "hello"),
        );
        state
            .enqueue_session_input(
                &info.id,
                "msg_pending_roundtrip",
                serde_json::json!({ "text": "follow up" }),
                2,
                "queue",
            )
            .await;

        let reloaded = AppState::with_database(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode", None),
            database.clone(),
        )
        .expect("reload");
        let stores = reloaded.stores.try_read().expect("stores");
        let record = stores.sessions.get(&info.id).expect("session loaded");
        assert_eq!(record.info.title, "Round trip");
        assert_eq!(record.messages.len(), 1);
        assert_eq!(record.messages[0]["id"], "msg_roundtrip");
        assert_eq!(
            reloaded.pending_session_input_ids(&info.id).await,
            ["msg_pending_roundtrip".to_string()].into_iter().collect()
        );
        drop(stores);
        assert!(reloaded.promote_next_session_queue(&info.id).await);
        let after_promotion = AppState::with_database(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode", None),
            database,
        )
        .expect("reload after promotion");
        assert!(after_promotion
            .pending_session_input_ids(&info.id)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn permission_reply_wakes_waiting_runner() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let waiting = state.clone();
        let task = tokio::spawn(async move {
            waiting
                .request_permission(
                    "ses_permission",
                    "edit",
                    vec!["src/main.rs".into()],
                    serde_json::json!({ "tool": "write" }),
                )
                .await
        });
        let request_id = loop {
            if let Some(id) = state.stores.read().await.permissions.keys().next().cloned() {
                break id;
            }
            tokio::task::yield_now().await;
        };
        assert!(
            state
                .resolve_permission(&request_id, &serde_json::json!({ "reply": "once" }))
                .await
        );
        assert!(task.await.expect("permission task"));
        assert!(state.stores.read().await.permissions.is_empty());
    }

    #[tokio::test]
    async fn rejecting_permission_rejects_all_pending_requests_in_session() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let mut events = state.events.subscribe();
        let first_state = state.clone();
        let first = tokio::spawn(async move {
            first_state
                .request_permission(
                    "ses_permission_reject",
                    "bash",
                    vec!["first".into()],
                    serde_json::json!({ "tool": "bash" }),
                )
                .await
        });
        let second_state = state.clone();
        let second = tokio::spawn(async move {
            second_state
                .request_permission(
                    "ses_permission_reject",
                    "edit",
                    vec!["second".into()],
                    serde_json::json!({ "tool": "edit" }),
                )
                .await
        });

        let request_ids = loop {
            let ids = state
                .stores
                .read()
                .await
                .permissions
                .iter()
                .filter(|(_, request)| {
                    request.get("sessionID").and_then(Value::as_str)
                        == Some("ses_permission_reject")
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            if ids.len() == 2 {
                break ids;
            }
            tokio::task::yield_now().await;
        };

        assert!(
            state
                .resolve_permission(&request_ids[0], &serde_json::json!({ "reply": "reject" }))
                .await
        );
        assert!(!first.await.expect("first permission task"));
        assert!(!second.await.expect("second permission task"));

        let replied = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|event| event.r#type == "permission.replied")
            .collect::<Vec<_>>();
        assert_eq!(replied.len(), 2);
        assert_eq!(
            replied
                .iter()
                .filter_map(|event| event.data.get("requestID").and_then(Value::as_str))
                .collect::<std::collections::HashSet<_>>(),
            request_ids.iter().map(String::as_str).collect()
        );
        assert!(replied.iter().all(|event| event.data["reply"] == "reject"));
        assert!(state.stores.read().await.permissions.is_empty());
        assert!(
            !state
                .resolve_permission(&request_ids[1], &serde_json::json!({ "reply": "once" }))
                .await
        );
    }

    #[tokio::test]
    async fn always_permission_reply_is_saved_for_future_tool_calls() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let waiting = state.clone();
        let task = tokio::spawn(async move {
            waiting
                .request_permission(
                    "ses_saved_permission",
                    "bash",
                    vec!["cargo test *".into()],
                    serde_json::json!({ "tool": "bash" }),
                )
                .await
        });
        let request_id = loop {
            if let Some(id) = state.stores.read().await.permissions.keys().next().cloned() {
                break id;
            }
            tokio::task::yield_now().await;
        };
        assert!(
            state
                .resolve_permission(&request_id, &serde_json::json!({ "reply": "always" }))
                .await
        );
        assert!(task.await.expect("permission task"));

        let stores = state.stores.read().await;
        let saved = &stores.saved_permissions;
        let saved = saved.values().next().expect("saved permission");
        assert_eq!(saved.action, "bash");
        assert_eq!(saved.resource, "cargo test *");
    }

    #[tokio::test]
    async fn saved_permissions_round_trip_through_sqlite() {
        let database = Arc::new(Database::open_memory().expect("database"));
        let state = AppState::with_database(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-permission", None),
            database.clone(),
        )
        .expect("state");
        let info = SessionInfo {
            id: "ses_permission_durable".into(),
            parent_id: None,
            project_id: "prj_permission_durable".into(),
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
            title: "Permission durable".into(),
            location: crate::schema::LocationRef {
                directory: "/tmp/opencode-permission".into(),
                workspace_id: None,
            },
            subpath: None,
            revert: None,
        };
        state.persist_session(&info);
        state.stores.write().await.sessions.insert(
            info.id.clone(),
            SessionRecord {
                info,
                messages: Vec::new(),
                active: false,
            },
        );

        let waiting = state.clone();
        let task = tokio::spawn(async move {
            waiting
                .request_permission(
                    "ses_permission_durable",
                    "bash",
                    vec!["cargo test *".into()],
                    serde_json::json!({ "tool": "bash" }),
                )
                .await
        });
        let request_id = loop {
            if let Some(id) = state.stores.read().await.permissions.keys().next().cloned() {
                break id;
            }
            tokio::task::yield_now().await;
        };
        assert!(
            state
                .resolve_permission(&request_id, &serde_json::json!({ "reply": "always" }))
                .await
        );
        assert!(task.await.expect("permission task"));

        let reloaded = AppState::with_database(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::with_directory("/tmp/opencode-permission", None),
            database,
        )
        .expect("reload");
        let stores = reloaded.stores.read().await;
        let saved = stores
            .saved_permissions
            .values()
            .next()
            .expect("saved permission after reload");
        assert_eq!(saved.project_id, "prj_permission_durable");
        assert_eq!(saved.action, "bash");
        assert_eq!(saved.resource, "cargo test *");
    }

    #[tokio::test]
    async fn session_runs_coalesce_and_cancel() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let token = state
            .acquire_session_run("ses_coalesce")
            .await
            .expect("first run owns slot");
        assert!(state.acquire_session_run("ses_coalesce").await.is_none());
        let next = state
            .finish_session_run("ses_coalesce")
            .await
            .expect("queued prompt gets a fresh pass");
        assert!(!next.is_cancelled());
        assert!(!token.is_cancelled());
        state.cancel_session_run("ses_coalesce").await;
        assert!(next.is_cancelled());
        assert!(state.finish_session_run("ses_coalesce").await.is_none());
    }

    #[tokio::test]
    async fn session_inputs_promote_steer_and_queue_in_order() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        state
            .enqueue_session_input("ses_input", "queue_1", serde_json::json!({}), 1, "queue")
            .await;
        state
            .enqueue_session_input("ses_input", "steer_2", serde_json::json!({}), 2, "steer")
            .await;
        assert!(state.pending_session_input("ses_input", "steer").await);
        assert!(state.pending_session_input("ses_input", "queue").await);
        assert_eq!(state.promote_session_steers("ses_input", 2).await, 1);
        assert!(!state.pending_session_input("ses_input", "steer").await);
        assert!(state.promote_next_session_queue("ses_input").await);
        assert!(!state.pending_session_input("ses_input", "queue").await);
        assert!(!state.promote_next_session_queue("ses_input").await);
    }

    #[tokio::test]
    async fn steer_interrupt_keeps_a_follow_up_run() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let token = state
            .acquire_session_run("ses_steer")
            .await
            .expect("active run");
        state.interrupt_session_for_steer("ses_steer").await;
        assert!(token.is_cancelled());
        let follow_up = state
            .finish_session_run("ses_steer")
            .await
            .expect("steer follow-up");
        assert!(!follow_up.is_cancelled());
        assert!(state.finish_session_run("ses_steer").await.is_none());
    }
}
