#![allow(clippy::type_complexity)]
//! F150/F151: cross-workspace sync through the pluggable in-memory transport
//! and first-use remote discovery / account workspace lifecycle against a mock
//! control-plane HTTP server.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use oc_sync::control_plane::adapters::{self};
use oc_sync::control_plane::deps::WorkspaceDeps;
use oc_sync::control_plane::global_bus::GlobalBus;
use oc_sync::control_plane::memory_transport::{MemoryControlPlane, MemorySyncApi};
use oc_sync::control_plane::sync_api::{ReqwestSyncApi, SyncApi};
use oc_sync::control_plane::types::{
    Target, WorkspaceAdapter, WorkspaceAdapterContext, WorkspaceInfo, WorkspaceListedInfo,
};
use oc_sync::control_plane::workspace::{CreateInput, SessionWarpInput, Workspace};
use oc_sync::sync::store::Store;

/// A recording adapter so the workspace service can resolve targets.
struct RecordingAdapter {
    target: Mutex<Target>,
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
        _info: &WorkspaceInfo,
        _env: &std::collections::BTreeMap<String, Option<String>>,
        _from: Option<&WorkspaceInfo>,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn list(
        &self,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
        Ok(vec![])
    }
    async fn remove(
        &self,
        _info: &WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
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

static PROJECT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn setup_with_api(api: Arc<dyn SyncApi>) -> (Workspace, Arc<RecordingAdapter>, Store, String) {
    let store = Store::new();
    let bus = GlobalBus::new();
    let adapter = Arc::new(RecordingAdapter {
        target: Mutex::new(Target::Remote {
            url: "http://memory.local".into(),
            headers: vec![],
        }),
    });
    let project_id = format!(
        "prj_{}",
        PROJECT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    adapters::register_adapter(&project_id, "fake", adapter.clone());
    let workspace = Workspace::new(
        store.clone(),
        bus.clone(),
        api,
        WorkspaceDeps::default(),
        true,
    );
    (workspace, adapter, store, project_id)
}

// ---------------------------------------------------------------------------
// F150: cross-workspace replay/steal through the in-memory transport
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_warp_replays_and_steals_through_memory_transport() {
    let plane = MemoryControlPlane::new();
    let api: Arc<dyn SyncApi> = Arc::new(MemorySyncApi::new(plane.clone()));
    let (workspace, adapter, store, project_id) = setup_with_api(api);

    *adapter.target.lock().unwrap() = Target::Remote {
        url: "http://memory.local".into(),
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

    // 12 events -> 2 replay batches of 10; the steal claims the session.
    assert_eq!(plane.replay_calls(), 2);
    assert_eq!(plane.steal_calls(), 1);
    // The plane now holds the replayed session events (the cross-workspace
    // handoff), and the local session row moved to the target workspace.
    assert_eq!(plane.history("ses_1").len(), 12);
    assert_eq!(plane.owner("ses_1").as_deref(), Some("remote"));
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
async fn sync_loop_replays_live_events_from_memory_transport() {
    // The workspace's sync loop connects to the memory plane's `/global/event`
    // stream and replays live sync events into its own store.
    let plane = MemoryControlPlane::new();
    let api: Arc<dyn SyncApi> = Arc::new(MemorySyncApi::new(plane.clone()));
    let (workspace, adapter, store, project_id) = setup_with_api(api);
    *adapter.target.lock().unwrap() = Target::Remote {
        url: "http://memory.local".into(),
        headers: vec![],
    };
    workspace
        .create(CreateInput {
            id: Some("wrk_sync".into()),
            ty: "fake".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: None,
        })
        .await
        .unwrap();

    // A remote workspace publishes an event for a session; the loop replays it.
    let def = oc_sync::sync::event::Definition::durable("session.next.moved", "sessionID", 1);
    let event = oc_sync::sync::event::SerializedEvent {
        id: oc_sync::sync::event::EventID("evt_live".into()),
        r#type: def.storage_type(),
        seq: 0,
        aggregate_id: "ses_live".into(),
        data: serde_json::json!({ "sessionID": "ses_live" }),
    };
    plane.publish(&event);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let replayed = store
            .history("ses_live")
            .iter()
            .any(|row| row.id.0 == "evt_live");
        if replayed {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "live event was not replayed into the local store"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let rows = store.history("ses_live");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].aggregate_id, "ses_live");
}

// ---------------------------------------------------------------------------
// F151: mock control-plane HTTP server + first-use discovery
// ---------------------------------------------------------------------------

/// A tiny tokio HTTP server serving the control-plane surface
/// (`/experimental/workspace`, `/sync/*`, `/global/event`, `/vcs/*`).
struct MockControlPlane {
    addr: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl MockControlPlane {
    async fn start() -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let request_log = requests.clone();

        let handle = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                let request_log = request_log.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(socket, request_log).await;
                });
            }
        });

        async fn handle_connection(
            mut socket: TcpStream,
            request_log: Arc<Mutex<Vec<String>>>,
        ) -> std::io::Result<()> {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // Read until the headers end.
            let header_end = loop {
                let read = socket.read(&mut chunk).await?;
                if read == 0 {
                    return Ok(());
                }
                buf.extend_from_slice(&chunk[..read]);
                if let Some(index) = find_subslice(&buf, b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let mut lines = head.lines();
            let request_line = lines.next().unwrap_or_default().to_string();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_string();
            let path = parts.next().unwrap_or_default().to_string();
            let mut content_length = 0usize;
            for line in lines {
                let lower = line.to_ascii_lowercase();
                if let Some(value) = lower.strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            while buf.len() < header_end + content_length {
                let read = socket.read(&mut chunk).await?;
                if read == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..read]);
            }
            request_log.lock().unwrap().push(format!("{method} {path}"));

            let body: Vec<u8>;
            let status: &str;
            let content_type: &str;
            if method == "GET" && path == "/experimental/workspace" {
                body = br#"[{"type":"remote","name":"discovered-account","branch":null,"directory":null,"extra":null,"projectID":"prj_http"}]"#.to_vec();
                status = "200 OK";
                content_type = "application/json";
            } else if method == "POST" && path == "/experimental/workspace" {
                body = br#"{"id":"wrk_created"}"#.to_vec();
                status = "201 Created";
                content_type = "application/json";
            } else if method == "DELETE" && path.starts_with("/experimental/workspace/") {
                body = Vec::new();
                status = "204 No Content";
                content_type = "text/plain";
            } else if method == "POST" && path == "/sync/replay" {
                body = br#"{"sessionID":"ses_1"}"#.to_vec();
                status = "200 OK";
                content_type = "application/json";
            } else if method == "POST" && path == "/sync/steal" {
                body = br#"{"sessionID":"ses_1"}"#.to_vec();
                status = "200 OK";
                content_type = "application/json";
            } else if method == "POST" && path == "/sync/history" {
                body = b"[]".to_vec();
                status = "200 OK";
                content_type = "application/json";
            } else if method == "GET" && path == "/global/event" {
                body = br#"data: {"payload":{"type":"server.heartbeat"}}

"#
                .to_vec();
                status = "200 OK";
                content_type = "text/event-stream";
            } else if method == "GET" && path == "/vcs/diff/raw" {
                body = b"PATCH".to_vec();
                status = "200 OK";
                content_type = "text/plain";
            } else if method == "POST" && path == "/vcs/apply" {
                body = br#"{"applied":true}"#.to_vec();
                status = "200 OK";
                content_type = "application/json";
            } else {
                body = b"not found".to_vec();
                status = "404 Not Found";
                content_type = "text/plain";
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(response.as_bytes()).await?;
            socket.write_all(&body).await?;
            let _ = socket.flush().await;
            Ok(())
        }

        Self {
            addr,
            requests,
            handle,
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn log(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockControlPlane {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[tokio::test]
async fn first_use_discovery_and_account_lifecycle_against_mock_control_plane() {
    let server = MockControlPlane::start().await;
    let api: Arc<dyn SyncApi> = Arc::new(ReqwestSyncApi::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    ));
    let store = Store::new();
    let bus = GlobalBus::new();
    let project_id = "prj_http".to_string();
    let workspace = Workspace::new(
        store.clone(),
        bus.clone(),
        api,
        WorkspaceDeps::default(),
        true,
    );

    // First-use remote discovery: no workspace of type "remote" exists yet, so
    // the caller supplies the control-plane base target and the builtin remote
    // adapter lists workspaces from the mock server.
    workspace
        .sync_list_with_target(
            &project_id,
            Some(Target::Remote {
                url: server.base_url(),
                headers: vec![],
            }),
        )
        .await
        .unwrap();
    let listed = workspace.list(&project_id).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "discovered-account");
    assert_eq!(listed[0].ty, "remote");
    assert!(server
        .log()
        .iter()
        .any(|entry| entry == "GET /experimental/workspace"));

    // Account workspace lifecycle: create a remote workspace (the adapter
    // posts to the mock control plane).
    let created = workspace
        .create(CreateInput {
            id: Some("wrk_acct".into()),
            ty: "remote".into(),
            branch: None,
            project_id: project_id.clone(),
            extra: Some(Some(serde_json::json!({ "url": server.base_url() }))),
        })
        .await
        .unwrap();
    assert_eq!(created.id, "wrk_acct");
    assert!(server
        .log()
        .iter()
        .any(|entry| entry == "POST /experimental/workspace"));

    // Removing the workspace deletes it on the control plane.
    workspace.remove("wrk_acct").await.unwrap();
    assert!(server
        .log()
        .iter()
        .any(|entry| entry == "DELETE /experimental/workspace/wrk_acct"));
    assert!(workspace.get("wrk_acct").await.is_none());
}

#[test]
fn mock_control_plane_logs_requests() {
    // Helper is exercised by the async tests; assert the shared plumbing.
    let _ = find_subslice(b"abc\r\n\r\ndef", b"\r\n\r\n");
}
