#![allow(clippy::type_complexity)]
#![allow(clippy::await_holding_lock)]
//! Workspace service integration tests against an in-memory store and a fake
//! remote sync surface.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oc_sync::control_plane::adapters::{self};
use oc_sync::control_plane::deps::WorkspaceDeps;
use oc_sync::control_plane::global_bus::GlobalBus;
use oc_sync::control_plane::sync_api::{
    Method, SyncApi, SyncHttpError, SyncHttpRequest, SyncHttpResponse,
};
use oc_sync::control_plane::types::{
    Target, WorkspaceAdapter, WorkspaceAdapterContext, WorkspaceInfo, WorkspaceListedInfo,
};
use oc_sync::control_plane::workspace::{CreateInput, SessionWarpInput, Workspace};
use oc_sync::control_plane::workspace_events::ConnectionStatus;
use oc_sync::sync::event::SerializedEvent;
use oc_sync::sync::store::Store;

/// A recording adapter so the workspace service can resolve targets.
struct RecordingAdapter {
    target: Mutex<Target>,
    created: Mutex<Vec<(String, std::collections::BTreeMap<String, Option<String>>)>>,
    removed: Mutex<Vec<String>>,
    listed: Mutex<Vec<WorkspaceListedInfo>>,
}

#[async_trait::async_trait]
impl WorkspaceAdapter for RecordingAdapter {
    fn name(&self) -> &'static str {
        "Fake"
    }
    fn description(&self) -> &'static str {
        "Fake adapter"
    }
    async fn configure(
        &self,
        info: WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<WorkspaceInfo> {
        Ok(info)
    }
    async fn create(
        &self,
        info: &WorkspaceInfo,
        env: &std::collections::BTreeMap<String, Option<String>>,
        _from: Option<&WorkspaceInfo>,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        self.created
            .lock()
            .unwrap()
            .push((info.name.clone(), env.clone()));
        Ok(())
    }
    async fn list(
        &self,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
        Ok(self.listed.lock().unwrap().clone())
    }
    async fn remove(
        &self,
        info: &WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        self.removed.lock().unwrap().push(info.id.clone());
        Ok(())
    }
    async fn target(
        &self,
        _info: &WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Target> {
        Ok(self.target.lock().unwrap().clone())
    }
}

/// A fake remote workspace server.
struct FakeSyncApi {
    history_events: Mutex<Vec<SerializedEvent>>,
    history_calls: Mutex<u32>,
    replay_calls: Mutex<u32>,
    steal_calls: Mutex<u32>,
}

impl FakeSyncApi {
    fn new() -> Self {
        Self {
            history_events: Mutex::new(Vec::new()),
            history_calls: Mutex::new(0),
            replay_calls: Mutex::new(0),
            steal_calls: Mutex::new(0),
        }
    }
}

#[async_trait::async_trait]
impl SyncApi for FakeSyncApi {
    async fn execute(&self, request: SyncHttpRequest) -> Result<SyncHttpResponse, SyncHttpError> {
        match (request.method, request.url.as_str()) {
            (Method::Post, url) if url.ends_with("/sync/history") => {
                *self.history_calls.lock().unwrap() += 1;
                let events = self.history_events.lock().unwrap().clone();
                Ok(SyncHttpResponse {
                    status: 200,
                    text: None,
                    json: Some(serde_json::to_value(events).unwrap()),
                })
            }
            (Method::Post, url) if url.ends_with("/sync/replay") => {
                *self.replay_calls.lock().unwrap() += 1;
                Ok(SyncHttpResponse {
                    status: 200,
                    text: None,
                    json: Some(serde_json::json!({ "sessionID": "ses_1" })),
                })
            }
            (Method::Post, url) if url.ends_with("/sync/steal") => {
                *self.steal_calls.lock().unwrap() += 1;
                Ok(SyncHttpResponse {
                    status: 200,
                    text: None,
                    json: Some(serde_json::json!({ "sessionID": "ses_1" })),
                })
            }
            (Method::Get, url) if url.ends_with("/vcs/diff/raw") => Ok(SyncHttpResponse {
                status: 200,
                text: Some("PATCH".into()),
                json: None,
            }),
            (Method::Post, url) if url.ends_with("/vcs/apply") => Ok(SyncHttpResponse {
                status: 200,
                text: None,
                json: Some(serde_json::json!({ "applied": true })),
            }),
            _ => Err(SyncHttpError::new("unexpected request", 404, None)),
        }
    }

    async fn event_stream(
        &self,
        _url: &str,
        _headers: &[(String, String)],
    ) -> Result<Box<dyn tokio::io::AsyncBufRead + Send + Unpin>, SyncHttpError> {
        Err(SyncHttpError::new("not implemented in test", 500, None))
    }
}

static PROJECT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn setup(
    experimental: bool,
) -> (
    Workspace,
    Arc<FakeSyncApi>,
    Arc<RecordingAdapter>,
    Store,
    GlobalBus,
    String,
) {
    let store = Store::new();
    let bus = GlobalBus::new();
    let api = Arc::new(FakeSyncApi::new());
    let adapter = Arc::new(RecordingAdapter {
        target: Mutex::new(Target::Remote {
            url: "http://localhost:9999/".into(),
            headers: vec![],
        }),
        created: Mutex::new(Vec::new()),
        removed: Mutex::new(Vec::new()),
        listed: Mutex::new(Vec::new()),
    });
    let project_id = format!(
        "prj_{}",
        PROJECT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    adapters::register_adapter(&project_id, "fake", adapter.clone());
    let workspace = Workspace::new(
        store.clone(),
        bus.clone(),
        api.clone(),
        WorkspaceDeps::default(),
        experimental,
    );
    (workspace, api, adapter, store, bus, project_id)
}

#[tokio::test]
async fn create_inserts_and_returns_workspace() {
    let (workspace, _, adapter, _, _, project_id) = setup(true);
    let info = workspace
        .create(CreateInput {
            id: None,
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();

    assert!(info.id.starts_with("wrk_"), "got {}", info.id);
    assert_eq!(info.ty, "fake");
    assert_eq!(info.project_id, project_id);
    assert!(info.time_used > 0);
    assert_eq!(adapter.created.lock().unwrap().len(), 1);
    let (name, env) = &adapter.created.lock().unwrap()[0];
    assert!(name.contains('-'));
    assert_eq!(
        env.get("OPENCODE_EXPERIMENTAL_WORKSPACES")
            .unwrap()
            .as_deref(),
        Some("true")
    );

    let listed = workspace.list(&project_id).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, info.id);

    let got = workspace.get(&info.id).await.unwrap();
    assert_eq!(got.name, info.name);
}

#[tokio::test]
async fn create_uses_provided_id_and_local_target_skips_remote() {
    let (workspace, api, adapter, _, _, project_id) = setup(true);
    *adapter.target.lock().unwrap() = Target::Local {
        directory: std::env::temp_dir()
            .join("oc-sync-test")
            .to_string_lossy()
            .into(),
    };
    let info = workspace
        .create(CreateInput {
            id: Some("wrk_given".into()),
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();
    assert_eq!(info.id, "wrk_given");
    assert_eq!(*api.history_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn remove_removes_workspace_and_calls_adapter() {
    let (workspace, _, adapter, _, _, project_id) = setup(true);
    let info = workspace
        .create(CreateInput {
            id: Some("wrk_rm".into()),
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();
    let removed = workspace.remove(&info.id).await.unwrap();
    assert_eq!(removed.id, "wrk_rm");
    assert_eq!(adapter.removed.lock().unwrap()[0], "wrk_rm");
    assert!(workspace.get("wrk_rm").await.is_none());
}

#[tokio::test]
async fn wait_for_sync_succeeds_when_already_synced() {
    let (workspace, _, _, store, _, _project_id) = setup(true);
    // Publish one event locally so the session cursor is 0.
    let def = oc_sync::sync::event::Definition::durable("session.next.moved", "sessionID", 1);
    store
        .publish(
            &def,
            serde_json::json!({ "sessionID": "ses_1" }),
            Default::default(),
        )
        .unwrap();
    let state = HashMap::from([("ses_1".to_string(), 0u64)]);
    let result = workspace.wait_for_sync("wrk_1", state, None, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn wait_for_sync_times_out_on_missing_cursor() {
    let (workspace, _, _, _, _, _project_id) = setup(true);
    let state = HashMap::from([("ses_1".to_string(), 5u64)]);
    let result = workspace
        .wait_for_sync("wrk_1", state, None, Some(Duration::from_millis(20)))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn status_reports_connection_state() {
    let (workspace, _, adapter, _, _, project_id) = setup(true);
    *adapter.target.lock().unwrap() = Target::Local {
        directory: std::env::temp_dir()
            .join("definitely-missing-oc-sync")
            .to_string_lossy()
            .into(),
    };
    workspace
        .create(CreateInput {
            id: Some("wrk_st".into()),
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();
    let statuses = workspace.status();
    let mine = statuses
        .iter()
        .find(|s| s.workspace_id == "wrk_st")
        .unwrap();
    assert_eq!(mine.status, ConnectionStatus::Error);
}

#[tokio::test]
async fn experimental_flag_disables_sync() {
    let (workspace, api, adapter, _, _, project_id) = setup(false);
    *adapter.target.lock().unwrap() = Target::Remote {
        url: "http://localhost:9999/".into(),
        headers: vec![],
    };
    workspace
        .create(CreateInput {
            id: Some("wrk_off".into()),
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();
    // No sync loop starts, so no status change is reported for the workspace.
    assert!(!workspace.is_syncing("wrk_off"));
    assert_eq!(*api.history_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn session_warp_replays_events_and_steals_session() {
    let (workspace, api, adapter, store, _, project_id) = setup(true);
    *adapter.target.lock().unwrap() = Target::Remote {
        url: "http://localhost:9999/".into(),
        headers: vec![],
    };
    workspace
        .create(CreateInput {
            id: Some("wrk_target".into()),
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();

    // A session with events in the source workspace.
    let def = oc_sync::sync::event::Definition::durable("session.next.moved", "sessionID", 1);
    for i in 0..12 {
        store
            .publish(
                &def,
                serde_json::json!({ "sessionID": "ses_1", "i": i }),
                Default::default(),
            )
            .unwrap();
    }
    {
        let sessions = workspace.sessions_store();
        sessions
            .lock()
            .unwrap()
            .push(oc_sync::control_plane::deps::SessionRow {
                id: "ses_1".into(),
                workspace_id: Some("wrk_old".into()),
                parent_id: None,
            });
    }

    // Old workspace must exist for the source sync history attempt.
    workspace
        .create(CreateInput {
            id: Some("wrk_old".into()),
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();

    workspace
        .session_warp(SessionWarpInput {
            workspace_id: Some("wrk_target".into()),
            session_id: "ses_1".into(),
            copy_changes: Some(true),
        })
        .await
        .unwrap();

    // 12 events -> 2 replay batches of 10.
    assert_eq!(*api.replay_calls.lock().unwrap(), 2);
    assert_eq!(*api.steal_calls.lock().unwrap(), 1);
    // The session now belongs to the target workspace.
    let sessions = workspace.sessions_store();
    let row = sessions
        .lock()
        .unwrap()
        .iter()
        .find(|row| row.id == "ses_1")
        .cloned()
        .unwrap();
    assert_eq!(row.workspace_id.as_deref(), Some("wrk_target"));
}

#[tokio::test]
async fn sync_list_discovers_workspaces_from_adapters() {
    let (workspace, _, adapter, _, _, project_id) = setup(true);
    adapter.listed.lock().unwrap().push(WorkspaceListedInfo {
        ty: "fake".into(),
        name: "discovered-1".into(),
        branch: Some(None),
        directory: Some(Some("/tmp/discovered".into())),
        extra: Some(None),
        project_id: project_id.clone(),
    });
    workspace.sync_list(&project_id).await.unwrap();
    let listed = workspace.list(&project_id).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "discovered-1");
    assert_eq!(listed[0].directory, Some(Some("/tmp/discovered".into())));
}
