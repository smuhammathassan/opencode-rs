//! The workspace service: create/list/remove workspaces, session warp, and the
//! remote sync loop that replays a workspace's session events.
//!
//! From reference/packages/opencode/src/control-plane/workspace.ts. The reference
//! runs on SQLite + effect; the port keeps the same orchestration over an
//! in-memory store (`sync::store::Store`) so the logic is testable. The remote
//! HTTP surface goes through `SyncApi`; the *server* side is oc-server scope.
//!
//! TODO(integration): back `db`/`sessions` with oc-database, and `deps` with
//! oc-session / oc-project / oc-provider services.

use std::collections::{HashMap, HashSet};

use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::sync::event::HistoryEvent;
use crate::sync::schema as id;
use crate::sync::store::{ReplayOptions, Store};

use super::adapters::{self};
use super::deps::{MemorySessionOps, MemorySessionStore, WorkspaceDeps};
use super::global_bus::{GlobalBus, GlobalEvent};
use super::slug;
use super::sync_api::{
    Method, ReplayPayload, ResponseKind, SessionPayload, SyncApi, SyncHttpError, SyncHttpRequest,
    SyncHttpResponse,
};
use super::types::{Target, WorkspaceInfo};
use super::util::{route, wait_event, WaitEventError};
use super::workspace_adapter_runtime as adapter_runtime;
use super::workspace_context::WorkspaceContext;
use super::workspace_events::{ConnectionStatus, StatusPayload, STATUS_TYPE};

/// `Info` from reference/packages/opencode/src/control-plane/workspace.ts:
/// `WorkspaceInfo` fields plus `timeUsed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Option<Value>>,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub time_used: u64,
}

impl Info {
    /// `fromRow` in the reference: nullable fields present as `null` when unset.
    fn from_row(row: &WorkspaceRow) -> Self {
        Self {
            id: row.id.clone(),
            ty: row.ty.clone(),
            name: row.name.clone(),
            branch: Some(row.branch.clone()),
            directory: Some(row.directory.clone()),
            extra: Some(row.extra.clone()),
            project_id: row.project_id.clone(),
            time_used: row.time_used,
        }
    }
}

/// `Info` is a `WorkspaceInfo` plus `timeUsed` in the reference
/// (`export type Info = WorkspaceInfo & { timeUsed: number }`).
impl From<&Info> for WorkspaceInfo {
    fn from(info: &Info) -> Self {
        WorkspaceInfo {
            id: info.id.clone(),
            ty: info.ty.clone(),
            name: info.name.clone(),
            branch: info.branch.clone(),
            directory: info.directory.clone(),
            extra: info.extra.clone(),
            project_id: info.project_id.clone(),
        }
    }
}

/// A `WorkspaceTable` row (reference/packages/core/src/control-plane/workspace.sql.ts).
#[derive(Debug, Clone)]
pub struct WorkspaceRow {
    pub id: String,
    pub ty: String,
    pub name: String,
    pub branch: Option<String>,
    pub directory: Option<String>,
    pub extra: Option<Value>,
    pub project_id: String,
    pub time_used: u64,
}

/// `CreateInput` from the reference.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<Option<String>>,
    #[serde(rename = "projectID")]
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Option<Value>>,
}

/// `SessionWarpInput` from the reference. `workspace_id: null` unwarp is
/// represented by `None`.
#[derive(Debug, Clone, Default)]
pub struct SessionWarpInput {
    pub workspace_id: Option<String>,
    pub session_id: String,
    pub copy_changes: Option<bool>,
}

/// `WorkspaceNotFoundError` from the reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WorkspaceNotFoundError {
    pub message: String,
    pub workspace_id: String,
}

/// `SessionEventsNotFoundError` from the reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SessionEventsNotFoundError {
    pub message: String,
    pub session_id: String,
}

/// `SessionWarpHttpError` from the reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SessionWarpHttpError {
    pub message: String,
    pub workspace_id: String,
    pub session_id: String,
    pub status: u16,
    pub body: String,
}

/// `SyncTimeoutError` from the reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SyncTimeoutError {
    pub message: String,
    pub state: HashMap<String, u64>,
}

/// The `SessionWarpError` union from the reference.
#[derive(Debug, thiserror::Error)]
pub enum SessionWarpError {
    #[error(transparent)]
    WorkspaceNotFound(#[from] WorkspaceNotFoundError),
    #[error(transparent)]
    SessionEventsNotFound(#[from] SessionEventsNotFoundError),
    #[error(transparent)]
    SessionWarpHttp(#[from] SessionWarpHttpError),
    #[error(transparent)]
    PatchApply(#[from] super::deps::PatchApplyError),
    #[error("session warp failed: {0}")]
    Other(anyhow::Error),
}

impl From<anyhow::Error> for SessionWarpError {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}

impl From<url::ParseError> for SessionWarpError {
    fn from(error: url::ParseError) -> Self {
        Self::Other(error.into())
    }
}

impl From<serde_json::Error> for SessionWarpError {
    fn from(error: serde_json::Error) -> Self {
        Self::Other(error.into())
    }
}

impl From<SyncHttpError> for SessionWarpError {
    fn from(error: SyncHttpError) -> Self {
        Self::Other(error.into())
    }
}

/// Wait errors for `wait_for_sync`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WaitForSyncError {
    #[error(transparent)]
    Timeout(#[from] SyncTimeoutError),
    #[error("sync aborted: {0}")]
    Aborted(String),
}

const TIMEOUT: Duration = Duration::from_millis(5000);

struct WorkspaceInner {
    db: Mutex<Vec<WorkspaceRow>>,
    sessions: MemorySessionStore,
    connections: std::sync::Mutex<HashMap<String, ConnectionStatus>>,
    sync_tasks: std::sync::Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    store: Store,
    bus: GlobalBus,
    api: Arc<dyn SyncApi>,
    deps: WorkspaceDeps,
    experimental_workspaces: bool,
}

#[derive(Clone)]
pub struct Workspace {
    inner: Arc<WorkspaceInner>,
}

impl Workspace {
    pub fn new(
        store: Store,
        bus: GlobalBus,
        api: Arc<dyn SyncApi>,
        deps: WorkspaceDeps,
        experimental_workspaces: bool,
    ) -> Self {
        let sessions: MemorySessionStore = Arc::new(std::sync::Mutex::new(Vec::new()));
        let deps = WorkspaceDeps {
            session: Arc::new(MemorySessionOps(sessions.clone())),
            ..deps
        };
        Self {
            inner: Arc::new(WorkspaceInner {
                db: Mutex::new(Vec::new()),
                sessions,
                connections: std::sync::Mutex::new(HashMap::new()),
                sync_tasks: std::sync::Mutex::new(HashMap::new()),
                store,
                bus,
                api,
                deps,
                experimental_workspaces,
            }),
        }
    }

    fn store(&self) -> &Store {
        &self.inner.store
    }

    /// The in-memory session table backing the service. Exposed for tests and
    /// the integration harness; removed when oc-database lands.
    /// TODO(integration): replace with the oc-session table.
    #[doc(hidden)]
    pub fn sessions_store(&self) -> MemorySessionStore {
        self.inner.sessions.clone()
    }

    fn bus(&self) -> &GlobalBus {
        &self.inner.bus
    }

    fn set_status(&self, id: &str, status: ConnectionStatus) {
        let mut connections = self.inner.connections.lock().expect("connections poisoned");
        let prev = connections.get(id).copied();
        if prev == Some(status) {
            return;
        }
        let next = StatusPayload {
            workspace_id: id.to_string(),
            status,
        };
        connections.insert(id.to_string(), status);
        drop(connections);
        // From the reference `setStatus`:
        // GlobalBus.emit("event", { directory: "global", workspace: id, payload: { type: Status.type, properties: next } })
        self.bus().emit(GlobalEvent {
            directory: Some("global".to_string()),
            project: None,
            workspace: Some(id.to_string()),
            payload: serde_json::json!({
                "type": STATUS_TYPE,
                "properties": next,
            }),
        });
    }

    /// `create` from the reference.
    pub async fn create(&self, input: CreateInput) -> anyhow::Result<Info> {
        let id = match &input.id {
            Some(existing) => id::ascending(id::Prefix::Workspace, Some(existing))?,
            None => id::ascending(id::Prefix::Workspace, None)?,
        };
        let adapter = adapters::get_adapter(&input.project_id, &input.ty)?;

        let config = adapter_runtime::configure(
            &adapter,
            WorkspaceInfo {
                id: id.clone(),
                ty: input.ty.clone(),
                name: slug::create(),
                branch: None,
                directory: Some(None),
                extra: Some(input.extra.clone().flatten()),
                project_id: input.project_id.clone(),
            },
        )
        .await?;

        let info = Info {
            id,
            ty: config.ty.clone(),
            name: config.name.clone(),
            branch: Some(config.branch.clone().flatten()),
            directory: Some(config.directory.clone().flatten()),
            extra: Some(config.extra.clone().flatten()),
            project_id: input.project_id.clone(),
            time_used: now_ms(),
        };

        self.insert_row(&info).await;

        let env = {
            let mut env = std::collections::BTreeMap::new();
            env.insert(
                "OPENCODE_AUTH_CONTENT".to_string(),
                Some(serde_json::to_string(&self.inner.deps.auth.all().await)?),
            );
            env.insert("OPENCODE_WORKSPACE_ID".to_string(), Some(config.id.clone()));
            env.insert(
                "OPENCODE_EXPERIMENTAL_WORKSPACES".to_string(),
                Some("true".to_string()),
            );
            env.insert(
                "OTEL_EXPORTER_OTLP_HEADERS".to_string(),
                std::env::var("OTEL_EXPORTER_OTLP_HEADERS").ok(),
            );
            env.insert(
                "OTEL_EXPORTER_OTLP_ENDPOINT".to_string(),
                std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
            );
            env.insert(
                "OTEL_RESOURCE_ATTRIBUTES".to_string(),
                std::env::var("OTEL_RESOURCE_ATTRIBUTES").ok(),
            );
            env
        };

        adapter_runtime::create(&adapter, &config, &env, None).await?;

        // From the reference: wait for the workspace to connect/error while
        // starting the sync loop.
        let info_for_event = info.clone();
        let bus = self.bus().clone();
        let wait = async move {
            let predicate = move |event: &GlobalEvent| {
                if event.workspace.as_deref() == Some(info_for_event.id.as_str())
                    && event.payload.get("type").and_then(|t| t.as_str()) == Some(STATUS_TYPE)
                {
                    let status = event
                        .payload
                        .get("properties")
                        .and_then(|p| p.get("status"))
                        .and_then(|s| s.as_str())
                        .unwrap_or_default();
                    return status == "error" || status == "connected";
                }
                false
            };
            let _ = wait_event(&bus, TIMEOUT, None, predicate).await;
        };
        let sync = self.start_sync(info.clone());
        let _ = tokio::join!(wait, sync);

        Ok(info)
    }

    /// `sessionWarp` from the reference: relocate a session between workspaces.
    pub async fn session_warp(&self, input: SessionWarpInput) -> Result<(), SessionWarpError> {
        let current = {
            let sessions = self.inner.sessions.lock().expect("session store poisoned");
            sessions
                .iter()
                .find(|row| row.id == input.session_id)
                .cloned()
        };

        if let Some(workspace_id) = current.as_ref().and_then(|row| row.workspace_id.clone()) {
            let previous = self.get(&workspace_id).await;
            if let Some(previous) = previous {
                let target = adapter_runtime::target(&WorkspaceInfo::from(&previous)).await?;
                match &target {
                    Target::Remote { url, headers } => {
                        if let Err(error) = self.sync_history(&previous, url, headers).await {
                            tracing::warn!(
                                workspaceID = %previous.id,
                                sessionID = %input.session_id,
                                error = %error,
                                "session warp final source sync failed"
                            );
                        }
                    }
                    Target::Local { .. } => {
                        self.inner.deps.prompt.cancel(&input.session_id).await?;
                    }
                }
                // Claim the session so future events from the old workspace are ignored.
                self.store().claim(
                    &input.session_id,
                    &input
                        .workspace_id
                        .clone()
                        .unwrap_or(previous.project_id.clone()),
                );
            }
        }

        let source_patch = if input.copy_changes.unwrap_or(false)
            && current
                .as_ref()
                .and_then(|r| r.workspace_id.clone())
                .is_some()
        {
            self.diff_raw_from(current.as_ref().and_then(|r| r.workspace_id.clone()))
                .await
        } else {
            String::new()
        };

        if !source_patch.is_empty() {
            // Apply file changes to the new workspace first; if it fails we don't warp.
            self.apply_patch_to(input.workspace_id.clone(), &source_patch)
                .await;
        }

        let Some(workspace_id) = input.workspace_id.clone() else {
            self.inner
                .deps
                .session
                .set_workspace(&input.session_id, None)
                .await?;
            return Ok(());
        };

        let Some(space) = self.get(&workspace_id).await else {
            return Err(WorkspaceNotFoundError {
                message: format!("Workspace not found: {workspace_id}"),
                workspace_id,
            }
            .into());
        };

        let target = adapter_runtime::target(&WorkspaceInfo::from(&space)).await?;
        if matches!(target, Target::Local { .. }) {
            self.inner
                .deps
                .session
                .set_workspace(&input.session_id, Some(workspace_id.clone()))
                .await?;
            return Ok(());
        }

        let rows = self.store().history(&input.session_id);
        if rows.is_empty() {
            return Err(SessionEventsNotFoundError {
                message: format!("No events found for session: {}", input.session_id),
                session_id: input.session_id.clone(),
            }
            .into());
        }

        let (url, headers) = match &target {
            Target::Remote { url, headers } => (url.clone(), headers.clone()),
            Target::Local { .. } => unreachable!("handled above"),
        };
        let directory = space.directory.clone().flatten().unwrap_or_default();

        for batch in rows.chunks(10) {
            let events: Vec<_> = batch
                .iter()
                .map(|row| crate::sync::event::SerializedEvent {
                    id: row.id.clone(),
                    r#type: row.r#type.clone(),
                    seq: row.seq,
                    aggregate_id: row.aggregate_id.clone(),
                    data: row.data.clone(),
                })
                .collect();
            let request = SyncHttpRequest {
                method: Method::Post,
                url: route(&url, "/sync/replay")?.to_string(),
                headers: headers.clone(),
                body: Some(serde_json::to_value(ReplayPayload {
                    directory: directory.clone(),
                    events,
                })?),
                response: ResponseKind::Json,
            };
            let response = self.inner.api.execute(request).await?;
            if !(200..300).contains(&response.status) {
                let body = response.text.clone().unwrap_or_default();
                return Err(SessionWarpHttpError {
                    message: format!(
                        "Failed to warp session {} into workspace {workspace_id}: HTTP {} {body}",
                        input.session_id, response.status
                    ),
                    workspace_id,
                    session_id: input.session_id.clone(),
                    status: response.status,
                    body,
                }
                .into());
            }
        }

        let request = SyncHttpRequest {
            method: Method::Post,
            url: route(&url, "/sync/steal")?.to_string(),
            headers: headers.clone(),
            body: Some(serde_json::to_value(SessionPayload {
                session_id: input.session_id.clone(),
            })?),
            response: ResponseKind::Json,
        };
        let response = self.inner.api.execute(request).await?;
        if !(200..300).contains(&response.status) {
            let body = response.text.clone().unwrap_or_default();
            return Err(SessionWarpHttpError {
                message: format!(
                    "Failed to steal session {} into workspace {workspace_id}: HTTP {} {body}",
                    input.session_id, response.status
                ),
                workspace_id,
                session_id: input.session_id.clone(),
                status: response.status,
                body,
            }
            .into());
        }

        self.inner
            .deps
            .session
            .set_workspace(&input.session_id, Some(workspace_id.clone()))
            .await?;

        Ok(())
    }

    /// `list` from the reference: workspaces for a project sorted by id.
    pub async fn list(&self, project_id: &str) -> Vec<Info> {
        let db = self.inner.db.lock().await;
        let mut rows: Vec<Info> = db
            .iter()
            .filter(|row| row.project_id == project_id)
            .map(Info::from_row)
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    /// `syncList` from the reference: discover workspaces from adapters and
    /// insert the ones not already known.
    pub async fn sync_list(&self, project_id: &str) -> anyhow::Result<()> {
        let mut names: std::collections::HashSet<String> = self
            .list(project_id)
            .await
            .into_iter()
            .map(|workspace| workspace.name)
            .collect();

        let mut discovered = Vec::new();
        for (ty, adapter) in adapters::registered_adapters(project_id) {
            match adapter_runtime::list(&adapter).await {
                Ok(items) => discovered.extend(items.into_iter().map(|item| (ty.clone(), item))),
                Err(error) => {
                    tracing::warn!(r#type = %ty, error = %error, "workspace adapter list failed");
                }
            }
        }

        for (_, item) in discovered {
            if names.contains(&item.name) {
                continue;
            }
            names.insert(item.name.clone());

            let info = Info {
                id: id::ascending(id::Prefix::Workspace, None)?,
                ty: item.ty,
                name: item.name,
                branch: item.branch,
                directory: item.directory,
                extra: item.extra,
                project_id: item.project_id,
                time_used: now_ms(),
            };
            self.insert_row(&info).await;
            let _ = self.start_sync(info).await;
        }
        Ok(())
    }

    /// `get` from the reference.
    pub async fn get(&self, id: &str) -> Option<Info> {
        let db = self.inner.db.lock().await;
        db.iter().find(|row| row.id == id).map(Info::from_row)
    }

    /// `remove` from the reference.
    pub async fn remove(&self, id: &str) -> Option<Info> {
        // From the reference: remove sessions that are not children of another
        // session in the same workspace.
        let sessions = {
            let sessions = self.inner.sessions.lock().expect("session store poisoned");
            let ids: HashSet<String> = sessions
                .iter()
                .filter(|row| row.workspace_id.as_deref() == Some(id))
                .map(|row| row.id.clone())
                .collect();
            sessions
                .iter()
                .filter(|row| {
                    row.workspace_id.as_deref() == Some(id)
                        && !(row.parent_id.is_some()
                            && ids.contains(row.parent_id.as_deref().unwrap_or_default()))
                })
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        };
        for session_id in sessions {
            if let Err(error) = self.inner.deps.session.remove(&session_id).await {
                tracing::warn!(sessionID = %session_id, error = %error, "failed to remove session");
            }
        }

        let row = {
            let mut db = self.inner.db.lock().await;
            let index = db.iter().position(|row| row.id == id);
            index.map(|index| db.remove(index))
        }?;

        self.stop_sync(id).await;

        let info = Info::from_row(&row);
        if let Err(error) = adapter_runtime::remove(&WorkspaceInfo::from(&info)).await {
            tracing::error!(r#type = %row.ty, error = %error, "adapter not available when removing workspace");
        }
        Some(info)
    }

    /// `status` from the reference.
    pub fn status(&self) -> Vec<StatusPayload> {
        let connections = self.inner.connections.lock().expect("connections poisoned");
        connections
            .iter()
            .map(|(id, status)| StatusPayload {
                workspace_id: id.clone(),
                status: *status,
            })
            .collect()
    }

    /// `isSyncing` from the reference.
    pub fn is_syncing(&self, workspace_id: &str) -> bool {
        let exists = self
            .inner
            .sync_tasks
            .lock()
            .expect("sync tasks poisoned")
            .contains_key(workspace_id);
        exists
            && self
                .inner
                .connections
                .lock()
                .expect("connections poisoned")
                .get(workspace_id)
                .copied()
                != Some(ConnectionStatus::Error)
    }

    /// `waitForSync` from the reference. `signal` mirrors the reference's
    /// `AbortSignal`.
    pub async fn wait_for_sync(
        &self,
        workspace_id: &str,
        state: HashMap<String, u64>,
        signal: Option<tokio_util::sync::CancellationToken>,
        timeout: Option<Duration>,
    ) -> Result<(), WaitForSyncError> {
        if self.synced(&state) {
            return Ok(());
        }
        let timeout = timeout.unwrap_or(TIMEOUT);
        loop {
            let predicate = |event: &GlobalEvent| {
                event.workspace.as_deref() == Some(workspace_id)
                    || event.payload.get("type").and_then(|t| t.as_str()) == Some("sync")
            };
            match wait_event(self.bus(), timeout, signal.clone(), predicate).await {
                Ok(()) => {
                    if self.synced(&state) {
                        return Ok(());
                    }
                }
                Err(WaitEventError::TimedOut) => {
                    return Err(SyncTimeoutError {
                        message: format!(
                            "Timed out waiting for sync fence: {}",
                            serde_json::to_string(&state).unwrap_or_default()
                        ),
                        state,
                    }
                    .into());
                }
                Err(WaitEventError::Aborted) => {
                    return Err(WaitForSyncError::Aborted("Request aborted".to_string()));
                }
            }
        }
    }

    /// `startWorkspaceSyncing` from the reference.
    pub async fn start_workspace_syncing(&self, project_id: &str) {
        let db = self.inner.db.lock().await;
        let rows: Vec<WorkspaceRow> = db
            .iter()
            .filter(|row| row.project_id == project_id)
            .cloned()
            .collect();
        drop(db);
        for row in rows {
            let me = self.clone();
            tokio::spawn(async move {
                let info = Info::from_row(&row);
                if let Err(error) = me.start_sync(info).await {
                    tracing::warn!(error = %error, "start workspace sync failed");
                    me.set_status(&row.id, ConnectionStatus::Error);
                }
            });
        }
    }

    async fn insert_row(&self, info: &Info) {
        let mut db = self.inner.db.lock().await;
        db.push(WorkspaceRow {
            id: info.id.clone(),
            ty: info.ty.clone(),
            name: info.name.clone(),
            branch: info.branch.clone().flatten(),
            directory: info.directory.clone().flatten(),
            extra: info.extra.clone().flatten(),
            project_id: info.project_id.clone(),
            time_used: info.time_used,
        });
    }

    /// `syncHistory` from the reference: pull new events for the workspace's
    /// sessions from the remote `/sync/history` endpoint and replay them.
    async fn sync_history(
        &self,
        space: &Info,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<(), SessionWarpError> {
        let session_ids = {
            let sessions = self.inner.sessions.lock().expect("session store poisoned");
            sessions
                .iter()
                .filter(|row| row.workspace_id.as_deref() == Some(space.id.as_str()))
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        };
        let state: HashMap<String, u64> = if session_ids.is_empty() {
            HashMap::new()
        } else {
            self.store()
                .sequences_for(&session_ids)
                .into_iter()
                .map(|(id, seq)| (id, seq.max(0) as u64))
                .collect()
        };

        let request = SyncHttpRequest {
            method: Method::Post,
            url: route(url, "/sync/history")?.to_string(),
            headers: headers.to_vec(),
            body: Some(serde_json::to_value(&state)?),
            response: ResponseKind::Json,
        };
        let response = self.inner.api.execute(request).await?;
        if !(200..300).contains(&response.status) {
            let body = response.text.clone().unwrap_or_default();
            return Err(SyncHttpError {
                message: format!("Workspace history HTTP failure: {} {body}", response.status),
                status: response.status,
                body: Some(body),
            }
            .into());
        }

        let history: Vec<HistoryEvent> = response
            .json
            .and_then(|json| serde_json::from_value(json).ok())
            .unwrap_or_default();
        for event in history {
            let replay_options = ReplayOptions {
                publish: true,
                owner_id: Some(space.id.clone()),
                strict_owner: false,
            };
            // The reference provides WorkspaceRef = space.id while replaying.
            let serialized = crate::sync::event::SerializedEvent {
                id: event.id,
                r#type: event.r#type,
                seq: event.seq,
                aggregate_id: event.aggregate_id,
                data: event.data,
            };
            let result = WorkspaceContext::restore(space.id.as_str(), async {
                self.store().replay(&serialized, &replay_options)
            })
            .await;
            if let Err(error) = result {
                tracing::warn!(workspaceID = %space.id, error = %error, "failed to replay history event");
            }
        }
        Ok(())
    }

    /// The infinite reconnect loop, mirroring `syncWorkspaceLoop`.
    async fn sync_workspace_loop(&self, space: Info) {
        let target = match adapter_runtime::target(&WorkspaceInfo::from(&space)).await {
            Ok(target) => target,
            Err(error) => {
                self.set_status(&space.id, ConnectionStatus::Error);
                tracing::warn!(workspace = %space.name, error = %error, "workspace target failed");
                return;
            }
        };
        let Target::Remote { url, headers } = target else {
            return;
        };

        let mut attempt: u32 = 0;
        loop {
            self.set_status(&space.id, ConnectionStatus::Connecting);

            let stream = match self.connect_and_sync(&space, &url, &headers).await {
                Ok(stream) => stream,
                Err(error) => {
                    self.set_status(&space.id, ConnectionStatus::Error);
                    tracing::warn!(workspace = %space.name, error = %error, "failed to connect to global sync");
                    sleep_backoff(&mut attempt).await;
                    continue;
                }
            };

            attempt = 0;
            self.set_status(&space.id, ConnectionStatus::Connected);
            self.consume_event_stream(stream, &space).await;
            self.set_status(&space.id, ConnectionStatus::Disconnected);

            sleep_backoff(&mut attempt).await;
        }
    }

    /// `connectSSE` + the `Effect.tap(() => syncHistory(...))` from the reference.
    async fn connect_and_sync(
        &self,
        space: &Info,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<Box<dyn tokio::io::AsyncBufRead + Send + Unpin>, anyhow::Error> {
        let event_url = route(url, "/global/event")?.to_string();
        let stream = self.inner.api.event_stream(&event_url, headers).await?;
        self.sync_history(space, url, headers).await?;
        Ok(stream)
    }

    /// `parseSSE` handling for the remote event stream: replay sync events and
    /// forward the rest to the global bus.
    async fn consume_event_stream(
        &self,
        mut reader: Box<dyn tokio::io::AsyncBufRead + Send + Unpin>,
        space: &Info,
    ) {
        let bus = self.bus().clone();
        let store = self.store().clone();
        let space_id = space.id.clone();
        let result = super::sse::parse_sse_stream(&mut reader, |evt: Value| {
            let bus = bus.clone();
            let store = store.clone();
            let space_id = space_id.clone();
            async move {
                // Mirrors the onEvent handler: skip non-object payloads and heartbeats.
                if !evt.is_object() || evt.get("payload").is_none() {
                    return Ok(());
                }
                let payload = evt.get("payload").cloned().unwrap_or(Value::Null);
                let payload_type = payload.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                if payload_type == "server.heartbeat" {
                    return Ok(());
                }
                if payload_type == "sync" {
                    if let Some(sync_event) = payload.get("syncEvent").cloned() {
                        let replay_options = ReplayOptions {
                            publish: true,
                            owner_id: Some(space_id.clone()),
                            strict_owner: false,
                        };
                        let serialized = match serde_json::from_value::<crate::sync::event::SerializedEvent>(sync_event) {
                            Ok(serialized) => serialized,
                            Err(error) => {
                                tracing::warn!(workspaceID = %space_id, error = %error, "failed to decode sync event");
                                return Ok(());
                            }
                        };
                        let failed = store.replay(&serialized, &replay_options).is_err();
                        if failed {
                            tracing::warn!(workspaceID = %space_id, "failed to replay global event");
                            return Ok(());
                        }
                    }
                }
                // Forward to the local global bus for other consumers.
                let directory = evt.get("directory").and_then(|d| d.as_str()).map(str::to_string);
                let project = evt.get("project").and_then(|p| p.as_str()).map(str::to_string);
                bus.emit(GlobalEvent {
                    directory,
                    project,
                    workspace: Some(space_id.clone()),
                    payload,
                });
                Ok(())
            }
        })
        .await;
        if let Err(error) = result {
            tracing::warn!(workspaceID = %space.id, error = %error, "failed to consume global event stream");
        }
    }

    /// `startSync` from the reference.
    async fn start_sync(&self, space: Info) -> anyhow::Result<()> {
        if !self.inner.experimental_workspaces {
            return Ok(());
        }
        let target = match adapter_runtime::target(&WorkspaceInfo::from(&space)).await {
            Ok(target) => target,
            Err(error) => {
                self.set_status(&space.id, ConnectionStatus::Error);
                tracing::warn!(workspaceID = %space.id, error = %error, "workspace target failed");
                return Ok(());
            }
        };
        match &target {
            Target::Local { directory } => {
                let exists = std::path::Path::new(directory).exists();
                self.set_status(
                    &space.id,
                    if exists {
                        ConnectionStatus::Connected
                    } else {
                        ConnectionStatus::Error
                    },
                );
            }
            Target::Remote { .. } => {
                let exists = self
                    .inner
                    .sync_tasks
                    .lock()
                    .expect("sync tasks poisoned")
                    .contains_key(&space.id);
                if exists
                    && self
                        .inner
                        .connections
                        .lock()
                        .expect("connections poisoned")
                        .get(&space.id)
                        .copied()
                        != Some(ConnectionStatus::Error)
                {
                    return Ok(());
                }
                self.set_status(&space.id, ConnectionStatus::Disconnected);
                let me = self.clone();
                let space_id = space.id.clone();
                let task_space_id = space_id.clone();
                let space_clone = space.clone();
                let task = tokio::spawn(async move {
                    me.sync_workspace_loop(space_clone).await;
                    me.inner
                        .sync_tasks
                        .lock()
                        .expect("sync tasks poisoned")
                        .remove(&task_space_id);
                });
                self.inner
                    .sync_tasks
                    .lock()
                    .expect("sync tasks poisoned")
                    .insert(space_id, task);
            }
        }
        Ok(())
    }

    /// `stopSync` from the reference.
    async fn stop_sync(&self, id: &str) {
        if let Some(task) = self
            .inner
            .sync_tasks
            .lock()
            .expect("sync tasks poisoned")
            .remove(id)
        {
            task.abort();
        }
        self.inner
            .connections
            .lock()
            .expect("connections poisoned")
            .remove(id);
    }

    /// The `synced` helper from the reference.
    fn synced(&self, state: &HashMap<String, u64>) -> bool {
        let ids: Vec<&String> = state.keys().collect();
        if ids.is_empty() {
            return true;
        }
        let done = self
            .store()
            .sequences_for(&ids.into_iter().cloned().collect::<Vec<_>>());
        state
            .iter()
            .all(|(id, seq)| done.get(id).copied().unwrap_or(-1) >= (*seq as i64))
    }

    /// `runInWorkspace` for the `vcs.diffRaw` case (text response, fallback `""`).
    async fn diff_raw_from(&self, workspace_id: Option<String>) -> String {
        let local = {
            let vcs = self.inner.deps.vcs.clone();
            Box::new(move || -> BoxFuture<'static, anyhow::Result<String>> {
                let vcs = vcs.clone();
                Box::pin(async move { vcs.diff_raw().await })
            }) as Box<dyn FnOnce() -> BoxFuture<'static, anyhow::Result<String>> + Send>
        };
        let remote = {
            |_workspace: &Info, target: &Target| match target {
                Target::Remote { url, headers } => SyncHttpRequest {
                    method: Method::Get,
                    url: route(url, "/vcs/diff/raw")
                        .map(|url| url.to_string())
                        .unwrap_or_default(),
                    headers: headers.clone(),
                    body: None,
                    response: ResponseKind::Text,
                },
                Target::Local { .. } => unreachable!(),
            }
        };
        self.run_in_workspace(workspace_id, local, remote, String::new(), |response| {
            response.text.clone()
        })
        .await
    }

    /// `runInWorkspace` for the `vcs.apply` case (json response, fallback
    /// `{ applied: false }`). Local errors are dropped in favor of the fallback
    /// to mirror the `catchIf(NotFoundError, ...)` style tolerance.
    async fn apply_patch_to(
        &self,
        workspace_id: Option<String>,
        patch: &str,
    ) -> super::deps::ApplyResult {
        let patch = patch.to_string();
        let local = {
            let vcs = self.inner.deps.vcs.clone();
            let patch = patch.clone();
            Box::new(
                move || -> BoxFuture<'static, anyhow::Result<super::deps::ApplyResult>> {
                    let vcs = vcs.clone();
                    let patch = patch.clone();
                    Box::pin(async move {
                        vcs.apply(&patch)
                            .await
                            .map_err(|error| anyhow::anyhow!("{error}"))
                    })
                },
            )
                as Box<
                    dyn FnOnce() -> BoxFuture<'static, anyhow::Result<super::deps::ApplyResult>>
                        + Send,
                >
        };
        let remote = {
            |_workspace: &Info, target: &Target| match target {
                Target::Remote { url, headers } => SyncHttpRequest {
                    method: Method::Post,
                    url: route(url, "/vcs/apply")
                        .map(|url| url.to_string())
                        .unwrap_or_default(),
                    headers: headers.clone(),
                    body: Some(serde_json::json!({ "patch": patch.clone() })),
                    response: ResponseKind::Json,
                },
                Target::Local { .. } => unreachable!(),
            }
        };
        self.run_in_workspace(
            workspace_id,
            local,
            remote,
            super::deps::ApplyResult { applied: false },
            |response| {
                response
                    .json
                    .clone()
                    .and_then(|json| serde_json::from_value::<super::deps::ApplyResult>(json).ok())
            },
        )
        .await
    }

    /// `runInWorkspace` from the reference.
    async fn run_in_workspace<A: Send + Clone>(
        &self,
        workspace_id: Option<String>,
        local: Box<dyn FnOnce() -> BoxFuture<'static, anyhow::Result<A>> + Send>,
        remote: impl Fn(&Info, &Target) -> SyncHttpRequest + Send + Sync,
        fallback: A,
        decode: impl Fn(&SyncHttpResponse) -> Option<A> + Send + Sync,
    ) -> A {
        let Some(workspace_id) = workspace_id else {
            return local().await.unwrap_or_else(|error| {
                tracing::warn!(error = %error, "workspace target request failed");
                fallback
            });
        };
        let Some(workspace) = self.get(&workspace_id).await else {
            return fallback;
        };
        let target = match adapter_runtime::target(&WorkspaceInfo::from(&workspace)).await {
            Ok(target) => target,
            Err(error) => {
                tracing::warn!(workspaceID = %workspace_id, error = %error, "workspace target failed");
                return fallback;
            }
        };
        if let Target::Local { directory } = &target {
            // The reference runs the local effect under `InstanceStore.provide({ directory })`.
            // TODO(integration): switch to running in `directory` once oc-project lands.
            let _ = directory;
            return local().await.unwrap_or_else(|error| {
                tracing::warn!(error = %error, "workspace target request failed");
                fallback
            });
        }
        let request = remote(&workspace, &target);
        let response = match self.inner.api.execute(request).await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(workspaceID = %workspace_id, error = %error, "workspace target request failed");
                return fallback;
            }
        };
        if !(200..300).contains(&response.status) {
            tracing::warn!(workspaceID = %workspace_id, status = response.status, "workspace target request failed");
            return fallback;
        }
        decode(&response).unwrap_or_else(|| {
            tracing::warn!(workspaceID = %workspace_id, "workspace target response decode failed");
            fallback
        })
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}

async fn sleep_backoff(attempt: &mut u32) {
    // Back off reconnect attempts up to 2 minutes while the workspace stays
    // unavailable.
    let ms = std::cmp::min(120_000u64, 1_000u64 << (*attempt).min(7));
    tokio::time::sleep(Duration::from_millis(ms)).await;
    *attempt += 1;
}
