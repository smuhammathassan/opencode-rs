//! End-to-end handler tests over the axum router.
//!
//! Golden assertions mirror the reference handler output (reference/packages/server/
//! src/handlers/* and reference/packages/opencode/src/server/routes/instance/httpapi/
//! handlers/*). Requests are dispatched with `tower::ServiceExt::oneshot` so no socket
//! is needed.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use futures::StreamExt;
use oc_database::tables::CredentialRow;
use oc_database::Database;
use oc_database::Value as SqlValue;
use oc_provider::provider::auth::{
    AuthCallbackResult, AuthHook, AuthOAuthResult, CallbackMethod, Method as AuthMethod,
    MethodType, OAuthCredential,
};
use oc_server::auth::AuthConfig;
use oc_server::cors::CorsOptions;
use oc_server::location::Location;
use oc_server::state::AppState;
use serde_json::Value;
use tokio::sync::Mutex;
use tower::ServiceExt;

static TEST_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn router() -> axum::Router {
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    oc_server::router::build(state)
}

fn router_with_events() -> (
    axum::Router,
    tokio::sync::broadcast::Receiver<oc_server::event::Event>,
) {
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let events = state.events.subscribe();
    (oc_server::router::build(state), events)
}

fn mcp_router(config: Value) -> axum::Router {
    let state = AppState::new_with_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        config,
    );
    oc_server::router::build(state)
}

fn durable_router(database: Arc<Database>) -> axum::Router {
    let state = AppState::with_database(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        database,
    )
    .expect("durable state");
    oc_server::router::build(state)
}

fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn request_owned(method: Method, uri: String) -> Request<Body> {
    request(method, &uri)
}

async fn send(router: &axum::Router, request: Request<Body>) -> Response {
    router.clone().oneshot(request).await.unwrap()
}

async fn json_body(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

struct ProviderAuthHook;

struct SyncListWorkspaceAdapter;

#[async_trait::async_trait]
impl oc_sync::control_plane::types::WorkspaceAdapter for SyncListWorkspaceAdapter {
    fn name(&self) -> &'static str {
        "API sync-list fixture"
    }

    fn description(&self) -> &'static str {
        "test workspace discovery"
    }

    async fn configure(
        &self,
        info: oc_sync::control_plane::types::WorkspaceInfo,
        _context: &oc_sync::control_plane::types::WorkspaceAdapterContext,
    ) -> anyhow::Result<oc_sync::control_plane::types::WorkspaceInfo> {
        Ok(info)
    }

    async fn create(
        &self,
        _info: &oc_sync::control_plane::types::WorkspaceInfo,
        _env: &BTreeMap<String, Option<String>>,
        _from: Option<&oc_sync::control_plane::types::WorkspaceInfo>,
        _context: &oc_sync::control_plane::types::WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn list(
        &self,
        context: &oc_sync::control_plane::types::WorkspaceAdapterContext,
    ) -> anyhow::Result<Vec<oc_sync::control_plane::types::WorkspaceListedInfo>> {
        Ok(vec![oc_sync::control_plane::types::WorkspaceListedInfo {
            ty: "api-sync-list".into(),
            name: "discovered-from-adapter".into(),
            branch: Some(None),
            directory: Some(Some("/tmp/discovered".into())),
            extra: Some(None),
            project_id: context
                .project_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("missing project context"))?,
        }])
    }

    async fn remove(
        &self,
        _info: &oc_sync::control_plane::types::WorkspaceInfo,
        _context: &oc_sync::control_plane::types::WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn target(
        &self,
        _info: &oc_sync::control_plane::types::WorkspaceInfo,
        _context: &oc_sync::control_plane::types::WorkspaceAdapterContext,
    ) -> anyhow::Result<oc_sync::control_plane::types::Target> {
        Ok(oc_sync::control_plane::types::Target::Local {
            directory: "/tmp/discovered".into(),
        })
    }
}

impl AuthHook for ProviderAuthHook {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod {
            r#type: MethodType::OAuth,
            label: "Test OAuth".into(),
            prompts: None,
        }]
    }

    fn validate(&self, _method_index: usize, _key: &str, _value: &str) -> Option<String> {
        None
    }

    fn authorize(
        &self,
        _method_index: usize,
        _inputs: &BTreeMap<String, String>,
    ) -> Result<AuthOAuthResult, anyhow::Error> {
        Ok(AuthOAuthResult {
            url: "https://auth.example.test/authorize".into(),
            method: CallbackMethod::Code,
            instructions: "Enter the code".into(),
        })
    }

    fn callback(&self, code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error> {
        if code == Some("fail") {
            return Ok(AuthCallbackResult::Failed);
        }
        Ok(AuthCallbackResult::Success {
            provider: None,
            oauth: Some(OAuthCredential {
                refresh: "refresh-from-hook".into(),
                access: "access-from-hook".into(),
                expires: 2_000,
                account_id: None,
                enterprise_url: None,
            }),
            api: None,
        })
    }
}

fn provider_auth_router() -> axum::Router {
    let state = AppState::new_with_provider_auth(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        BTreeMap::from([(
            "test-provider".into(),
            Box::new(ProviderAuthHook) as Box<dyn AuthHook>,
        )]),
    );
    oc_server::router::build(state)
}

#[derive(Clone)]
struct ShareMockState(Arc<Mutex<Vec<Value>>>);

async fn share_mock_create(
    axum::extract::State(state): axum::extract::State<ShareMockState>,
    axum::Json(body): axum::Json<Value>,
) -> axum::Json<Value> {
    state.0.lock().await.push(serde_json::json!({
        "method": "POST",
        "path": "/api/share",
        "body": body,
    }));
    axum::Json(serde_json::json!({
        "id": "share_test",
        "url": "https://opncd.ai/share/share_test",
        "secret": "secret_test",
    }))
}

async fn share_mock_sync(
    axum::extract::State(state): axum::extract::State<ShareMockState>,
    axum::extract::Path(share_id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> StatusCode {
    state.0.lock().await.push(serde_json::json!({
        "method": "POST",
        "path": format!("/api/share/{share_id}/sync"),
        "body": body,
    }));
    StatusCode::NO_CONTENT
}

async fn share_mock_remove(
    axum::extract::State(state): axum::extract::State<ShareMockState>,
    axum::extract::Path(share_id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> StatusCode {
    state.0.lock().await.push(serde_json::json!({
        "method": "DELETE",
        "path": format!("/api/share/{share_id}"),
        "body": body,
    }));
    StatusCode::NO_CONTENT
}

async fn share_next_mock_create(
    axum::extract::State(state): axum::extract::State<ShareMockState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> axum::Json<Value> {
    state.0.lock().await.push(serde_json::json!({
        "method": "POST",
        "path": "/api/shares",
        "authorization": headers.get("authorization").and_then(|value| value.to_str().ok()),
        "org": headers.get("x-org-id").and_then(|value| value.to_str().ok()),
        "body": body,
    }));
    axum::Json(serde_json::json!({
        "id": "share_next_test",
        "url": "https://share.example.test/share/share_next_test",
    }))
}

async fn share_next_mock_sync(
    axum::extract::State(state): axum::extract::State<ShareMockState>,
    axum::extract::Path(share_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> StatusCode {
    state.0.lock().await.push(serde_json::json!({
        "method": "POST",
        "path": format!("/api/shares/{share_id}/sync"),
        "authorization": headers.get("authorization").and_then(|value| value.to_str().ok()),
        "org": headers.get("x-org-id").and_then(|value| value.to_str().ok()),
        "body": body,
    }));
    StatusCode::NO_CONTENT
}

async fn share_next_mock_remove(
    axum::extract::State(state): axum::extract::State<ShareMockState>,
    axum::extract::Path(share_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> StatusCode {
    state.0.lock().await.push(serde_json::json!({
        "method": "DELETE",
        "path": format!("/api/shares/{share_id}"),
        "authorization": headers.get("authorization").and_then(|value| value.to_str().ok()),
        "org": headers.get("x-org-id").and_then(|value| value.to_str().ok()),
    }));
    StatusCode::NO_CONTENT
}

#[tokio::test]
async fn health_returns_healthy_true() {
    let router = router();
    let response = send(&router, request(Method::GET, "/api/health")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        serde_json::json!({ "healthy": true })
    );
}

#[tokio::test]
async fn tui_control_routes_deliver_requests_and_responses() {
    let router = router();
    let response = send(&router, request(Method::POST, "/tui/open-help")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, serde_json::json!(true));

    let next = send(&router, request(Method::GET, "/tui/control/next")).await;
    assert_eq!(next.status(), StatusCode::OK);
    assert_eq!(
        json_body(next).await,
        serde_json::json!({ "path": "/tui/open-help", "body": {} })
    );

    let response = Request::builder()
        .method(Method::POST)
        .uri("/tui/control/response")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"accepted":true}"#))
        .unwrap();
    let response = send(&router, response).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, serde_json::json!(true));
    assert_eq!(
        oc_server::shared::tui_control::next_tui_response().await,
        Some(serde_json::json!({ "accepted": true }))
    );
}

#[tokio::test]
async fn sync_routes_replay_claim_and_read_durable_history() {
    let router = router();
    let start = send(&router, request(Method::POST, "/sync/start")).await;
    assert_eq!(start.status(), StatusCode::OK);
    assert_eq!(json_body(start).await, serde_json::json!(true));

    let replay = Request::builder()
        .method(Method::POST)
        .uri("/sync/replay")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"directory":"/tmp","events":[{"id":"evt_sync_1","type":"session.next.agent.switched.1","seq":0,"aggregateID":"ses_sync","data":{"sessionID":"ses_sync","agent":"build"}}]}"#,
        ))
        .unwrap();
    let replay = send(&router, replay).await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(
        json_body(replay).await,
        serde_json::json!({ "sessionID": "ses_sync" })
    );

    let history = Request::builder()
        .method(Method::POST)
        .uri("/sync/history")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ses_sync":-1}"#))
        .unwrap();
    let history = send(&router, history).await;
    assert_eq!(history.status(), StatusCode::OK);
    assert_eq!(
        json_body(history).await,
        serde_json::json!([{
            "id": "evt_sync_1",
            "aggregate_id": "ses_sync",
            "seq": 0,
            "type": "session.next.agent.switched.1",
            "data": { "sessionID": "ses_sync", "agent": "build" }
        }])
    );

    let steal = Request::builder()
        .method(Method::POST)
        .uri("/sync/steal")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"sessionID":"ses_sync"}"#))
        .unwrap();
    let steal = send(&router, steal).await;
    assert_eq!(steal.status(), StatusCode::OK);
    assert_eq!(
        json_body(steal).await,
        serde_json::json!({ "sessionID": "ses_sync" })
    );
}

#[tokio::test]
async fn workspace_routes_manage_local_workspace_projection_and_warp() {
    let router = router();
    let session = Request::builder()
        .method(Method::POST)
        .uri("/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let session = send(&router, session).await;
    assert_eq!(session.status(), StatusCode::OK);
    let session_id = json_body(session).await["id"]
        .as_str()
        .expect("session id")
        .to_string();

    let create = Request::builder()
        .method(Method::POST)
        .uri("/experimental/workspace")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"id":"wrk_test","type":"worktree","name":"feature","directory":"/tmp/feature"}"#,
        ))
        .unwrap();
    let create = send(&router, create).await;
    assert_eq!(create.status(), StatusCode::OK);
    assert_eq!(json_body(create).await["id"], "wrk_test");

    let list = send(&router, request(Method::GET, "/experimental/workspace")).await;
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await[0]["name"], "feature");

    let warp = Request::builder()
        .method(Method::POST)
        .uri("/experimental/workspace/warp")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            "{{\"sessionID\":\"{session_id}\",\"workspaceID\":\"wrk_test\"}}"
        )))
        .unwrap();
    let warp = send(&router, warp).await;
    assert_eq!(warp.status(), StatusCode::NO_CONTENT);

    let move_session = Request::builder()
        .method(Method::POST)
        .uri("/experimental/control-plane/move-session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            "{{\"sessionID\":\"{session_id}\",\"workspaceID\":\"wrk_test\"}}"
        )))
        .unwrap();
    let move_session = send(&router, move_session).await;
    assert_eq!(move_session.status(), StatusCode::NO_CONTENT);

    let status = send(
        &router,
        request(Method::GET, "/experimental/workspace/status"),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(json_body(status).await[0]["status"], "ready");

    let remove = Request::builder()
        .method(Method::DELETE)
        .uri("/experimental/workspace/wrk_test")
        .body(Body::empty())
        .unwrap();
    let remove = send(&router, remove).await;
    assert_eq!(remove.status(), StatusCode::OK);
    assert_eq!(json_body(remove).await, serde_json::json!(true));
}

#[tokio::test]
async fn workspace_sync_list_projects_registered_adapter_discovery() {
    let location = Location::default_location();
    oc_sync::control_plane::adapters::register_adapter(
        &location.project_id,
        "api-sync-list",
        Arc::new(SyncListWorkspaceAdapter),
    );
    let state = AppState::new(AuthConfig::default(), CorsOptions::default(), location);
    let router = oc_server::router::build(state);

    let sync = Request::builder()
        .method(Method::POST)
        .uri("/experimental/workspace/sync-list")
        .body(Body::empty())
        .unwrap();
    let sync = send(&router, sync).await;
    assert_eq!(sync.status(), StatusCode::NO_CONTENT);

    let list = send(&router, request(Method::GET, "/experimental/workspace")).await;
    assert_eq!(list.status(), StatusCode::OK);
    let workspaces = json_body(list).await;
    assert!(workspaces.as_array().unwrap().iter().any(|workspace| {
        workspace["name"] == "discovered-from-adapter" && workspace["type"] == "api-sync-list"
    }));
}

#[tokio::test]
async fn pty_create_runs_command_and_captures_output() {
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let router = oc_server::router::build(state.clone());
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/pty")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"command":"printf pty-ok"}"#))
        .unwrap();
    let response = send(&router, create).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let pty_id = body["data"]["id"].as_str().unwrap().to_string();

    let duplicate = Request::builder()
        .method(Method::POST)
        .uri("/api/pty")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            r#"{{"id":"{pty_id}","command":"true"}}"#
        )))
        .unwrap();
    let duplicate = send(&router, duplicate).await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    // The capture task is intentionally detached from the create response;
    // allow a little scheduling headroom when the full API suite is busy.
    let mut captured = false;
    for _ in 0..200 {
        let found = state
            .stores
            .read()
            .await
            .pty
            .get(&pty_id)
            .map(|record| String::from_utf8_lossy(&record.buffer).contains("pty-ok"))
            .unwrap_or(false);
        if found {
            captured = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(captured, "PTY command output was not captured");

    let update = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/pty/{pty_id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"cols":100,"rows":40}"#))
        .unwrap();
    let response = send(&router, update).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["data"]["cols"], 100);
    assert_eq!(body["data"]["rows"], 40);
}

#[tokio::test]
async fn create_session_matches_reference_shape() {
    let router = router();
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"agent":"build"}"#))
        .unwrap();
    let response = send(&router, create).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let data = &body["data"];
    assert!(data["id"].as_str().unwrap().starts_with("ses_"));
    assert_eq!(data["agent"], "build");
    assert_eq!(data["cost"], 0.0);
    assert_eq!(data["tokens"]["input"], 0.0);
    assert_eq!(data["time"]["created"].as_i64().unwrap() > 0, true);
    assert_eq!(data["title"], "New Session");
    assert!(data["location"]["directory"].as_str().is_some());
    assert_eq!(data["projectID"].as_str().unwrap().len() > 0, true);
}

#[tokio::test]
async fn project_current_uses_oc_project_resolution() {
    let router = router();
    let response = send(&router, request(Method::GET, "/project/current")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(body["worktree"]
        .as_str()
        .is_some_and(|path| !path.is_empty()));
    assert!(body["time"]["created"]
        .as_u64()
        .is_some_and(|time| time > 0));
}

#[tokio::test]
async fn experimental_worktree_routes_use_git_worktree_service() {
    let _test_home_guard = TEST_HOME_LOCK.lock().unwrap();
    let directory =
        std::env::temp_dir().join(format!("opencode-rs-worktree-api-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let run_git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&directory)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    };
    run_git(&["init", "--quiet"]);
    std::fs::write(directory.join("README.md"), "worktree\n").unwrap();
    run_git(&["add", "README.md"]);
    run_git(&[
        "-c",
        "user.name=OpenCode Test",
        "-c",
        "user.email=opencode-test@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "initial",
    ]);
    let directory = std::fs::canonicalize(&directory).unwrap();
    let test_home =
        std::env::temp_dir().join(format!("opencode-rs-worktree-home-{}", std::process::id()));
    std::fs::create_dir_all(&test_home).unwrap();
    let previous_test_home = std::env::var_os("OPENCODE_TEST_HOME");
    std::env::set_var("OPENCODE_TEST_HOME", &test_home);

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(&directory.to_string_lossy(), None),
    );
    let router = oc_server::router::build(state);
    let create = Request::builder()
        .method(Method::POST)
        .uri("/experimental/worktree")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":"feature"}"#))
        .unwrap();
    let created = json_body(send(&router, create).await).await;
    assert_eq!(created["name"], "feature");
    assert_eq!(created["branch"], "opencode/feature");
    let worktree_directory = std::fs::canonicalize(created["directory"].as_str().unwrap())
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let listed =
        json_body(send(&router, request(Method::GET, "/experimental/worktree")).await).await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["directory"] == worktree_directory),
        "listed={listed:?} expected={worktree_directory}"
    );

    let remove = Request::builder()
        .method(Method::DELETE)
        .uri("/experimental/worktree")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({"directory": worktree_directory}).to_string(),
        ))
        .unwrap();
    let removed = json_body(send(&router, remove).await).await;
    assert_eq!(removed, true);

    std::fs::remove_dir_all(&directory).unwrap();
    std::fs::remove_dir_all(&test_home).unwrap();
    if let Some(previous_test_home) = previous_test_home {
        std::env::set_var("OPENCODE_TEST_HOME", previous_test_home);
    } else {
        std::env::remove_var("OPENCODE_TEST_HOME");
    }
}

#[tokio::test]
async fn reference_and_project_copy_handlers_use_local_services() {
    let root = std::env::temp_dir().join(format!(
        "opencode-rs-reference-copy-api-{}",
        std::process::id()
    ));
    let source = root.join("source");
    let copy = root.join("copy");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("README.md"), "reference copy\n").unwrap();
    let state = AppState::new_with_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(source.to_str().unwrap(), None),
        serde_json::json!({
            "references": { "docs": { "path": source.to_string_lossy() } }
        }),
    );
    let router = oc_server::router::build(state);

    let references = json_body(send(&router, request(Method::GET, "/api/reference")).await).await;
    assert_eq!(references["data"][0]["name"], "docs");
    assert_eq!(references["data"][0]["source"]["type"], "local");

    let create = Request::builder()
        .method(Method::POST)
        .uri("/experimental/project/global/copy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "strategy": "copy",
                "directory": copy,
            })
            .to_string(),
        ))
        .unwrap();
    let response = send(&router, create).await;
    assert_eq!(response.status(), StatusCode::OK);
    let created = json_body(response).await;
    let copy_text = copy.to_string_lossy().to_string();
    assert_eq!(
        created["data"]["directory"].as_str(),
        Some(copy_text.as_str())
    );
    assert_eq!(
        std::fs::read_to_string(copy.join("README.md")).unwrap(),
        "reference copy\n"
    );

    let remove = Request::builder()
        .method(Method::DELETE)
        .uri("/experimental/project/global/copy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "directory": copy, "force": true }).to_string(),
        ))
        .unwrap();
    assert_eq!(send(&router, remove).await.status(), StatusCode::NO_CONTENT);
    assert!(!copy.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn session_share_and_unshare_follow_remote_protocol_and_persist_durably() {
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let mock = axum::Router::new()
        .route("/api/share", axum::routing::post(share_mock_create))
        .route(
            "/api/share/:shareID/sync",
            axum::routing::post(share_mock_sync),
        )
        .route(
            "/api/share/:shareID",
            axum::routing::delete(share_mock_remove),
        )
        .with_state(ShareMockState(calls.clone()));
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("share mock listener failed: {error}"),
    };
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let mock_task = tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });

    let database = Arc::new(Database::open_memory().unwrap());
    let state = AppState::with_database_and_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        database.clone(),
        serde_json::json!({ "enterprise": { "url": base_url } }),
    )
    .unwrap();
    let router = oc_server::router::build(state);
    let created = json_body(
        send(
            &router,
            Request::builder()
                .method(Method::POST)
                .uri("/session")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await,
    )
    .await;
    let session_id = created["id"].as_str().unwrap().to_string();

    let shared = send(
        &router,
        request(Method::POST, &format!("/session/{session_id}/share")),
    )
    .await;
    assert_eq!(shared.status(), StatusCode::OK);
    assert_eq!(
        json_body(shared).await["url"],
        "https://opncd.ai/share/share_test"
    );

    let persisted = database
        .get_by::<oc_database::tables::SessionShareRow>(
            "session_share",
            "session_id",
            &oc_database::Value::Text(session_id.clone()),
            oc_database::tables::json_columns("session_share"),
        )
        .unwrap()
        .expect("share row persisted");
    assert_eq!(persisted.id, "share_test");
    assert_eq!(persisted.secret, "secret_test");

    for _ in 0..200 {
        if calls.lock().await.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let calls_before_remove = calls.lock().await.clone();
    assert_eq!(calls_before_remove[0]["method"], "POST");
    assert_eq!(calls_before_remove[0]["path"], "/api/share");
    assert_eq!(
        calls_before_remove[0]["body"],
        serde_json::json!({
            "sessionID": session_id,
        })
    );
    assert_eq!(calls_before_remove[1]["path"], "/api/share/share_test/sync");
    assert_eq!(calls_before_remove[1]["body"]["secret"], "secret_test");
    assert!(calls_before_remove[1]["body"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["type"] == "session"));

    let removed = send(
        &router,
        request(Method::DELETE, &format!("/session/{session_id}/share")),
    )
    .await;
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    assert!(database
        .get_by::<oc_database::tables::SessionShareRow>(
            "session_share",
            "session_id",
            &oc_database::Value::Text(session_id.clone()),
            oc_database::tables::json_columns("session_share"),
        )
        .unwrap()
        .is_none());

    let calls_after_remove = calls.lock().await.clone();
    assert_eq!(calls_after_remove[2]["method"], "DELETE");
    assert_eq!(calls_after_remove[2]["path"], "/api/share/share_test");
    assert_eq!(
        calls_after_remove[2]["body"],
        serde_json::json!({
            "secret": "secret_test",
        })
    );
    mock_task.abort();
}

#[tokio::test]
async fn account_share_uses_bearer_org_headers_and_account_resource() {
    let _env_lock = TEST_HOME_LOCK.lock().unwrap();
    let previous_token = std::env::var_os("OPENCODE_TEST_ACCOUNT_TOKEN");
    std::env::set_var("OPENCODE_TEST_ACCOUNT_TOKEN", "token_test");

    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let mock = axum::Router::new()
        .route("/api/shares", axum::routing::post(share_next_mock_create))
        .route(
            "/api/shares/:shareID/sync",
            axum::routing::post(share_next_mock_sync),
        )
        .route(
            "/api/shares/:shareID",
            axum::routing::delete(share_next_mock_remove),
        )
        .with_state(ShareMockState(calls.clone()));
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("share-next mock listener failed: {error}"),
    };
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let mock_task = tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });

    let database = Arc::new(Database::open_memory().unwrap());
    let state = AppState::with_database_and_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        database,
        serde_json::json!({
            "shareNext": {
                "url": base_url,
                "orgID": "org_test",
                "tokenEnv": "OPENCODE_TEST_ACCOUNT_TOKEN"
            }
        }),
    )
    .unwrap();
    let router = oc_server::router::build(state);
    let created = json_body(
        send(
            &router,
            Request::builder()
                .method(Method::POST)
                .uri("/session")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await,
    )
    .await;
    let session_id = created["id"].as_str().unwrap().to_string();

    let shared = send(
        &router,
        request(Method::POST, &format!("/session/{session_id}/share")),
    )
    .await;
    assert_eq!(shared.status(), StatusCode::OK);
    assert_eq!(
        json_body(shared).await["url"],
        "https://share.example.test/share/share_next_test"
    );

    for _ in 0..200 {
        if calls.lock().await.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let created_calls = calls.lock().await.clone();
    assert_eq!(created_calls[0]["path"], "/api/shares");
    assert_eq!(created_calls[0]["authorization"], "Bearer token_test");
    assert_eq!(created_calls[0]["org"], "org_test");
    assert!(created_calls[1]["body"].get("secret").is_none());

    let removed = send(
        &router,
        request(Method::DELETE, &format!("/session/{session_id}/share")),
    )
    .await;
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    let calls_after_remove = calls.lock().await.clone();
    assert_eq!(calls_after_remove[2]["path"], "/api/shares/share_next_test");
    assert_eq!(calls_after_remove[2]["authorization"], "Bearer token_test");
    assert_eq!(calls_after_remove[2]["org"], "org_test");

    mock_task.abort();
    match previous_token {
        Some(value) => std::env::set_var("OPENCODE_TEST_ACCOUNT_TOKEN", value),
        None => std::env::remove_var("OPENCODE_TEST_ACCOUNT_TOKEN"),
    }
}

#[tokio::test]
async fn session_list_returns_cursor() {
    let router = router();
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let created = json_body(send(&router, create).await).await;
    let session_id = created["data"]["id"].as_str().unwrap().to_string();

    let response = send(&router, request(Method::GET, "/api/session")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["id"], session_id);
    assert!(body["cursor"].is_object());
}

#[tokio::test]
async fn prompt_returns_admitted_input() {
    let (router, mut events) = router_with_events();
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let created = json_body(send(&router, create).await).await;
    let session_id = created["data"]["id"].as_str().unwrap();

    let prompt = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/session/{session_id}/prompt"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"prompt":{"text":"hello"}}"#))
        .unwrap();
    let response = send(&router, prompt).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let data = &body["data"];
    assert!(data["id"].as_str().unwrap().starts_with("msg_"));
    assert_eq!(data["sessionID"], session_id);
    assert_eq!(data["delivery"], "steer");
    assert_eq!(data["prompt"]["text"], "hello");
    assert_eq!(data["admittedSeq"], 1);

    let prompted = events.recv().await.expect("prompted event");
    let admitted = events.recv().await.expect("admitted event");
    assert_eq!(prompted.r#type, "session.next.prompted");
    assert_eq!(admitted.r#type, "session.next.prompt.admitted");
    assert_eq!(prompted.data["sessionID"], session_id);
    assert_eq!(prompted.data["delivery"], "steer");
    assert_eq!(admitted.data["messageID"], prompted.data["messageID"]);

    let history = send(
        &router,
        request_owned(
            Method::GET,
            format!("/api/session/{session_id}/history?after=-1&limit=10"),
        ),
    )
    .await;
    let history = json_body(history).await;
    assert_eq!(history["data"].as_array().unwrap().len(), 2);
    assert_eq!(history["data"][0]["type"], "session.next.prompted");
    assert_eq!(history["data"][1]["type"], "session.next.prompt.admitted");
}

#[tokio::test]
async fn prompt_for_missing_session_is_404() {
    let router = router();
    let prompt = Request::builder()
        .method(Method::POST)
        .uri("/api/session/ses_missing/prompt")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"prompt":{"text":"hello"}}"#))
        .unwrap();
    let response = send(&router, prompt).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_body(response).await;
    assert_eq!(body["_tag"], "SessionNotFoundError");
    assert_eq!(body["sessionID"], "ses_missing");
}

#[tokio::test]
async fn message_roundtrip() {
    let router = router();
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let created = json_body(send(&router, create).await).await;
    let session_id = created["data"]["id"].as_str().unwrap();

    let prompt = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/session/{session_id}/prompt"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"prompt":{"text":"hi"}}"#))
        .unwrap();
    let prompted = json_body(send(&router, prompt).await).await;
    let message_id = prompted["data"]["id"].as_str().unwrap();

    let response = send(
        &router,
        request_owned(
            Method::GET,
            format!("/api/session/{session_id}/message/{message_id}"),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["data"]["id"], message_id);
    assert_eq!(body["data"]["type"], "user");
    assert_eq!(body["data"]["text"], "hi");

    let list = send(
        &router,
        request_owned(Method::GET, format!("/api/session/{session_id}/message")),
    )
    .await;
    let list_body = json_body(list).await;
    assert_eq!(list_body["data"].as_array().unwrap().len(), 1);
    assert!(list_body["cursor"]["next"].as_str().is_some());
}

#[tokio::test]
async fn durable_session_and_message_survive_state_reload() {
    let database = Arc::new(Database::open_memory().expect("database"));
    let router = durable_router(database.clone());
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"Durable"}"#))
        .unwrap();
    let created = json_body(send(&router, create).await).await;
    let session_id = created["data"]["id"].as_str().unwrap().to_string();

    let prompt = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/session/{session_id}/prompt"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"prompt":{"text":"persist me"}}"#))
        .unwrap();
    let prompted = json_body(send(&router, prompt).await).await;
    let message_id = prompted["data"]["id"].as_str().unwrap().to_string();

    let reloaded = durable_router(database);
    let listed = json_body(send(&reloaded, request(Method::GET, "/api/session")).await).await;
    assert_eq!(listed["data"][0]["id"], session_id);
    assert_eq!(listed["data"][0]["title"], "New Session");

    let message = send(
        &reloaded,
        request_owned(
            Method::GET,
            format!("/api/session/{session_id}/message/{message_id}"),
        ),
    )
    .await;
    assert_eq!(message.status(), StatusCode::OK);
    let message = json_body(message).await;
    assert_eq!(message["data"]["text"], "persist me");
}

#[tokio::test]
async fn session_title_mutation_uses_session_service_and_survives_reload() {
    let database = Arc::new(Database::open_memory().expect("database"));
    let router = durable_router(database.clone());

    let created = json_body(
        send(
            &router,
            Request::builder()
                .method(Method::POST)
                .uri("/session")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await,
    )
    .await;
    let session_id = created["id"].as_str().unwrap().to_string();

    let updated = send(
        &router,
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/session/{session_id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"title":"Renamed by service"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(json_body(updated).await["title"], "Renamed by service");

    let reloaded = durable_router(database);
    let persisted = send(
        &reloaded,
        request_owned(Method::GET, format!("/api/session/{session_id}")),
    )
    .await;
    assert_eq!(persisted.status(), StatusCode::OK);
    assert_eq!(
        json_body(persisted).await["data"]["title"],
        "Renamed by service"
    );
}

#[tokio::test]
async fn session_fork_persists_child_history_without_moving_parent_messages() {
    let database = Arc::new(Database::open_memory().expect("database"));
    let router = durable_router(database.clone());

    let created = json_body(
        send(
            &router,
            Request::builder()
                .method(Method::POST)
                .uri("/api/session")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await,
    )
    .await;
    let source_id = created["data"]["id"].as_str().unwrap().to_string();

    for (id, text) in [("msg_fork_first", "first"), ("msg_fork_second", "second")] {
        let response = send(
            &router,
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/session/{source_id}/prompt"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"id":"{id}","prompt":{{"text":"{text}"}}}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let forked = send(
        &router,
        Request::builder()
            .method(Method::POST)
            .uri(format!("/api/session/{source_id}/fork"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"messageID":"msg_fork_first"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(forked.status(), StatusCode::OK);
    let forked = json_body(forked).await;
    let child_id = forked["data"]["id"].as_str().unwrap().to_string();
    assert_eq!(forked["data"]["parentID"], source_id);
    assert_eq!(forked["data"]["title"], "New Session (fork #1)");

    let source_message = database
        .get_message("msg_fork_first", &source_id)
        .unwrap()
        .expect("source message remains durable");
    assert_eq!(source_message.data["text"], "first");
    let child_messages = database
        .list_messages_page(&child_id, 50, None)
        .unwrap()
        .into_iter()
        .map(|message| message.data)
        .collect::<Vec<_>>();
    assert_eq!(child_messages.len(), 1);
    assert_eq!(child_messages[0]["text"], "first");
    assert_ne!(child_messages[0]["id"], "msg_fork_first");

    let reloaded = durable_router(database);
    let child = send(
        &reloaded,
        request_owned(Method::GET, format!("/api/session/{child_id}")),
    )
    .await;
    assert_eq!(child.status(), StatusCode::OK);
    assert_eq!(json_body(child).await["data"]["parentID"], source_id);

    let child_history = send(
        &reloaded,
        request_owned(Method::GET, format!("/api/session/{child_id}/message")),
    )
    .await;
    assert_eq!(child_history.status(), StatusCode::OK);
    let child_history = json_body(child_history).await;
    assert_eq!(child_history["data"].as_array().unwrap().len(), 1);
    assert_eq!(child_history["data"][0]["text"], "first");
}

#[tokio::test]
async fn missing_message_is_404_tagged() {
    let router = router();
    let response = send(
        &router,
        request(Method::GET, "/api/session/ses_x/message/msg_x"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_body(response).await;
    assert_eq!(body["_tag"], "SessionNotFoundError");
}

#[tokio::test]
async fn event_stream_emits_connected_first() {
    let router = router();
    let response = send(&router, request(Method::GET, "/api/event")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(
        response.headers()["cache-control"],
        "no-cache, no-transform"
    );
    assert_eq!(response.headers()["x-accel-buffering"], "no");

    let frame = read_sse_first_frame(response.into_body()).await;
    assert!(frame.starts_with("event: message\n"), "frame: {frame}");
    let json = frame
        .trim_start_matches("event: message\ndata: ")
        .trim_end();
    let payload: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(payload["type"], "server.connected");
    assert_eq!(payload["data"], serde_json::json!({}));
}

#[tokio::test]
async fn v1_event_stream_emits_compat_connected_first() {
    let router = router();
    let response = send(&router, request(Method::GET, "/event")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let frame = read_sse_first_frame(response.into_body()).await;
    let json = frame
        .trim_start_matches("event: message\ndata: ")
        .trim_end();
    let payload: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(payload["type"], "server.connected");
    assert_eq!(payload["properties"], serde_json::json!({}));
    assert!(payload["data"].is_null());
}

#[tokio::test]
async fn config_get_returns_config() {
    let router = router();
    let response = send(&router, request(Method::GET, "/config")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body.is_object());
}

#[tokio::test]
async fn config_update_persists_project_config() {
    let directory = std::env::temp_dir().join(format!(
        "opencode-rs-config-test-{}-{}",
        std::process::id(),
        oc_server::event::event_id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(&directory.to_string_lossy(), None),
    );
    let router = oc_server::router::build(state.clone());
    let update = Request::builder()
        .method(Method::PATCH)
        .uri("/config")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"model":"stub/demo"}"#))
        .unwrap();
    let response = send(&router, update).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["model"], "stub/demo");

    let persisted = std::fs::read_to_string(directory.join("opencode.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&persisted).unwrap()["model"],
        "stub/demo"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn config_update_preserves_jsonc_comments_and_trailing_comma() {
    let directory = std::env::temp_dir().join(format!(
        "opencode-rs-jsonc-config-test-{}-{}",
        std::process::id(),
        oc_server::event::event_id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("opencode.jsonc");
    std::fs::write(
        &path,
        "{\n  // Keep this project note.\n  \"model\": \"stub/old\",\n  // Keep this unrelated setting.\n  \"theme\": \"dark\",\n}\n",
    )
    .unwrap();

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(&directory.to_string_lossy(), None),
    );
    let router = oc_server::router::build(state);
    let update = Request::builder()
        .method(Method::PATCH)
        .uri("/config")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"model":"stub/new","theme":"light","newKey":true}"#,
        ))
        .unwrap();
    let response = send(&router, update).await;
    assert_eq!(response.status(), StatusCode::OK);

    let persisted = std::fs::read_to_string(&path).unwrap();
    assert!(persisted.contains("Keep this project note"));
    assert!(persisted.contains("Keep this unrelated setting"));
    assert!(persisted.contains("\"model\": \"stub/new\""));
    assert!(persisted.contains("\"newKey\": true"));
    assert!(persisted.ends_with("}\n"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn config_update_preserves_nested_jsonc_comments() {
    let directory = std::env::temp_dir().join(format!(
        "opencode-rs-nested-jsonc-config-test-{}-{}",
        std::process::id(),
        oc_server::event::event_id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("opencode.jsonc");
    std::fs::write(
        &path,
        r#"{
  "provider": {
    // Keep the provider note.
    "openai": {
      "options": {
        // Keep the options note.
        "baseURL": "https://old.example.test",
      },
      "models": {
        // Keep the model note.
        "demo": {
          "name": "Old Demo",
        },
      },
    },
  },
}
"#,
    )
    .unwrap();

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(&directory.to_string_lossy(), None),
    );
    let router = oc_server::router::build(state);
    let update = Request::builder()
        .method(Method::PATCH)
        .uri("/config")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"provider":{"openai":{"options":{"baseURL":"https://new.example.test"},"models":{"demo":{"name":"New Demo"}}}}}"#,
        ))
        .unwrap();
    let response = send(&router, update).await;
    assert_eq!(response.status(), StatusCode::OK);

    let persisted = std::fs::read_to_string(&path).unwrap();
    assert!(persisted.contains("Keep the provider note"));
    assert!(persisted.contains("Keep the options note"));
    assert!(persisted.contains("Keep the model note"));
    assert!(persisted.contains("https://new.example.test"));
    assert!(persisted.contains("New Demo"));
    let parsed = oc_plugin::jsonc::parse(&persisted).expect("nested JSONC remains valid");
    assert_eq!(
        parsed.value["provider"]["openai"]["options"]["baseURL"],
        "https://new.example.test"
    );
    assert_eq!(
        parsed.value["provider"]["openai"]["models"]["demo"]["name"],
        "New Demo"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn capability_lists_are_wired_to_local_registries() {
    let router = router();

    let agents = json_body(send(&router, request(Method::GET, "/api/agent")).await).await;
    assert_eq!(agents["data"][0]["id"], "build");

    let commands = json_body(send(&router, request(Method::GET, "/api/command")).await).await;
    assert!(commands["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["name"] == "init"));
    assert!(commands["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["name"] == "review"));

    let skills = json_body(send(&router, request(Method::GET, "/api/skill")).await).await;
    assert!(skills["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|skill| skill["name"] == "customize-opencode"));

    let tools = json_body(send(&router, request(Method::GET, "/experimental/tool")).await).await;
    assert!(tools
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "read"));
    let tool_ids =
        json_body(send(&router, request(Method::GET, "/experimental/tool/ids")).await).await;
    assert!(tool_ids
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "apply_patch"));
}

#[tokio::test]
async fn command_list_and_execution_include_project_skills() {
    let directory = std::env::temp_dir().join(format!(
        "opencode-rs-command-skill-test-{}-{}",
        std::process::id(),
        oc_server::event::event_id()
    ));
    let skill = directory.join(".opencode/skills/review-notes/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        &skill,
        "---\nname: review-notes\ndescription: Review notes\n---\nReview $ARGUMENTS\n",
    )
    .unwrap();
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(&directory.to_string_lossy(), None),
    );
    let router = oc_server::router::build(state);
    let commands = json_body(send(&router, request(Method::GET, "/api/command")).await).await;
    assert!(commands["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["name"] == "review-notes"));

    let session = json_body(
        send(
            &router,
            Request::builder()
                .method(Method::POST)
                .uri("/session")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"model":{"providerID":"stub","modelID":"demo"}}"#,
                ))
                .unwrap(),
        )
        .await,
    )
    .await;
    let session_id = session["id"].as_str().unwrap();
    let response = send(
        &router,
        Request::builder()
            .method(Method::POST)
            .uri(format!("/session/{session_id}/command"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"command":"review-notes","arguments":"the diff"}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let admitted = json_body(response).await;
    let admitted_text = admitted["parts"][0]["text"].as_str().unwrap();
    assert!(admitted_text.starts_with("Review the diff"));
    assert!(admitted_text.contains("Base directory for this skill: "));
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn question_reply_resolves_waiting_question_service() {
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let (handle, request_info) = state.question_service.ask("ses_question", vec![], None);
    let request_id = request_info.id.to_string();
    state.stores.write().await.questions.insert(
        request_id.clone(),
        serde_json::json!({
            "id": request_id,
            "sessionID": "ses_question",
            "questions": []
        }),
    );
    let router = oc_server::router::build(state);
    let waiter = tokio::spawn(async move { handle.await });
    let reply = Request::builder()
        .method(Method::POST)
        .uri(format!("/question/{request_id}/reply"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"answers":[["yes"]]}"#))
        .unwrap();
    let response = send(&router, reply).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, serde_json::Value::Bool(true));
    assert_eq!(
        waiter.await.unwrap().unwrap(),
        vec![vec!["yes".to_string()]]
    );
}

#[tokio::test]
async fn compact_endpoint_persists_runner_checkpoint() {
    let router = router();
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let created = json_body(send(&router, create).await).await;
    let session_id = created["data"]["id"].as_str().unwrap();

    let response = send(
        &router,
        request(Method::POST, &format!("/session/{session_id}/compact")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let messages = json_body(
        send(
            &router,
            request(Method::GET, &format!("/session/{session_id}/message")),
        )
        .await,
    )
    .await;
    assert!(messages
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["type"] == "compaction"));
}

#[tokio::test]
async fn session_interrupt_emits_idle_status_after_cancelling_run() {
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let mut events = state.events.subscribe();
    let router = oc_server::router::build(state);
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let created = json_body(send(&router, create).await).await;
    let session_id = created["data"]["id"].as_str().unwrap().to_string();

    let response = send(
        &router,
        request(
            Method::POST,
            &format!("/api/session/{session_id}/interrupt"),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(std::iter::from_fn(|| events.try_recv().ok()).any(|event| {
        event.r#type == "session.status"
            && event.data["sessionID"] == session_id
            && event.data["status"]["type"] == "idle"
    }));
}

#[tokio::test]
async fn provider_and_model_catalogs_have_location_wrappers() {
    let router = router();
    let providers = json_body(send(&router, request(Method::GET, "/api/provider")).await).await;
    assert!(providers["location"]["directory"].is_string());
    assert!(providers["data"].is_array());

    let models = json_body(send(&router, request(Method::GET, "/api/model")).await).await;
    assert!(models["location"]["directory"].is_string());
    assert!(models["data"].is_array());
}

#[tokio::test]
async fn legacy_provider_list_reports_connected_ids_and_default_models() {
    let state = AppState::new_with_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        serde_json::json!({
            "provider": {
                "local": {
                    "name": "Local Gateway",
                    "options": {"baseURL": "http://127.0.0.1:9999/v1"},
                    "models": {
                        "demo": {"name": "Demo Model"}
                    }
                }
            }
        }),
    );
    let router = oc_server::router::build(state);
    let providers = json_body(send(&router, request(Method::GET, "/provider")).await).await;

    assert!(providers["connected"]
        .as_array()
        .unwrap()
        .iter()
        .any(|provider| provider == "local"));
    assert_eq!(providers["default"]["local"], "demo");
}

#[tokio::test]
async fn custom_provider_config_reaches_provider_and_model_catalogs() {
    let state = AppState::new_with_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        serde_json::json!({
            "provider": {
                "local": {
                    "name": "Local Gateway",
                    "options": {"baseURL": "http://127.0.0.1:9999/v1"},
                    "models": {
                        "demo": {"name": "Demo Model"}
                    }
                }
            }
        }),
    );
    let router = oc_server::router::build(state);

    let providers = json_body(send(&router, request(Method::GET, "/api/provider")).await).await;
    assert!(providers["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|provider| provider["id"] == "local"));

    let models = json_body(send(&router, request(Method::GET, "/api/model")).await).await;
    assert!(models["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["id"] == "demo" && model["providerID"] == "local"));
}

#[tokio::test]
async fn configured_agents_are_exposed_to_run_clients() {
    let state = AppState::new_with_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        serde_json::json!({
            "agent": {
                "review": {
                    "description": "Review code changes",
                    "mode": "primary",
                    "prompt": "Review the current change set."
                },
                "explore": {
                    "description": "Explore the repository",
                    "mode": "subagent"
                }
            }
        }),
    );
    let router = oc_server::router::build(state);
    let agents = json_body(send(&router, request(Method::GET, "/agent")).await).await;
    let data = agents.as_array().unwrap();
    assert!(data
        .iter()
        .any(|agent| { agent["name"] == "review" && agent["mode"] == "primary" }));
    assert!(data
        .iter()
        .any(|agent| { agent["name"] == "explore" && agent["mode"] == "subagent" }));

    let v2_agents = json_body(send(&router, request(Method::GET, "/api/agent")).await).await;
    assert!(v2_agents["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|agent| { agent["id"] == "review" && agent["mode"] == "primary" }));
}

#[tokio::test]
async fn v2_revert_commit_restores_project_snapshot() {
    let _test_home_guard = TEST_HOME_LOCK.lock().unwrap();
    let directory =
        std::env::temp_dir().join(format!("opencode-rs-snapshot-api-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&directory)
        .status()
        .unwrap();
    let file = directory.join("tracked.txt");
    std::fs::write(&file, "before\n").unwrap();
    let directory = std::fs::canonicalize(&directory).unwrap();
    let test_home =
        std::env::temp_dir().join(format!("opencode-rs-snapshot-home-{}", std::process::id()));
    let previous_test_home = std::env::var_os("OPENCODE_TEST_HOME");
    std::fs::create_dir_all(&test_home).unwrap();
    std::env::set_var("OPENCODE_TEST_HOME", &test_home);

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(&directory.to_string_lossy(), None),
    );
    let context = state
        .project_runtime
        .load(&directory.to_string_lossy())
        .await
        .unwrap();
    let snapshot = state
        .project_runtime
        .snapshot
        .track(&context)
        .await
        .unwrap_or_else(|| panic!("git snapshot should be tracked; context={context:?}"));
    state.project_runtime.dispose(&context).await;
    assert!(
        !snapshot.is_empty(),
        "snapshot was empty; context={context:?}"
    );

    std::fs::write(&file, "after\n").unwrap();
    let router = oc_server::router::build(state.clone());
    let create = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"agent":"build"}"#))
        .unwrap();
    let session = json_body(send(&router, create).await).await;
    let session_id = session["data"]["id"].as_str().unwrap().to_string();

    let message = serde_json::json!({"id":"msg_snapshot"});
    state
        .stores
        .write()
        .await
        .sessions
        .get_mut(&session_id)
        .unwrap()
        .messages
        .push(message.clone());

    let stage = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/session/{session_id}/revert/stage"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({"messageID": message["id"], "snapshot": snapshot}).to_string(),
        ))
        .unwrap();
    let response = send(&router, stage).await;
    assert_eq!(response.status(), StatusCode::OK);

    let commit = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/session/{session_id}/revert/commit"))
        .body(Body::empty())
        .unwrap();
    let response = send(&router, commit).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "before\n");
    std::fs::remove_dir_all(&directory).unwrap();
    std::fs::remove_dir_all(&test_home).unwrap();
    if let Some(previous_test_home) = previous_test_home {
        std::env::set_var("OPENCODE_TEST_HOME", previous_test_home);
    } else {
        std::env::remove_var("OPENCODE_TEST_HOME");
    }
}

#[tokio::test]
async fn v1_search_and_vcs_surfaces_use_local_services() {
    let router = router();

    let files = json_body(
        send(
            &router,
            request(Method::GET, "/find/file?query=instance_handlers&limit=10"),
        )
        .await,
    )
    .await;
    assert!(
        files
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == "src/instance_handlers.rs"),
        "files={files}"
    );

    let matches = json_body(
        send(
            &router,
            request(Method::GET, "/find?pattern=prompt_text&limit=10"),
        )
        .await,
    )
    .await;
    assert!(matches
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["entry"]["path"] == "src/instance_handlers.rs"));

    let vcs = json_body(send(&router, request(Method::GET, "/vcs")).await).await;
    assert_eq!(vcs["command"], "git");
    assert!(vcs["state"]["mode"].is_string());
}

#[tokio::test]
async fn global_health_has_version() {
    let router = router();
    let response = send(&router, request(Method::GET, "/global/health")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["healthy"], true);
    assert_eq!(body["version"], oc_server::version());
}

#[tokio::test]
async fn doc_serves_openapi() {
    let router = router();
    let response = send(&router, request(Method::GET, "/doc")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["info"]["title"], "opencode");
    assert!(body["paths"].get("/api/session").is_some());
}

#[tokio::test]
async fn unknown_route_is_404() {
    let router = router();
    let response = send(&router, request(Method::GET, "/nope")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mcp_auth_start_uses_real_oauth_service_errors() {
    let router = mcp_router(serde_json::json!({
        "mcp": {
            "remote": { "type": "remote", "url": "not a url" }
        }
    }));
    let response = send(&router, request(Method::POST, "/mcp/remote/auth")).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = json_body(response).await;
    assert_eq!(body["_tag"], "UnknownError");
    assert!(body["message"]
        .as_str()
        .unwrap_or_default()
        .contains("Invalid MCP URL"));
}

#[tokio::test]
async fn cors_preflight_allows_localhost_origin() {
    let router = router();
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/session")
        .header(header::ORIGIN, "http://localhost:5173")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(Body::empty())
        .unwrap();
    let response = send(&router, preflight).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "http://localhost:5173"
    );
    assert_eq!(response.headers()["access-control-max-age"], "86400");
}

fn authed_router() -> axum::Router {
    let auth = AuthConfig {
        username: "opencode".into(),
        password: Some("secret".into()),
    };
    let state = AppState::new(auth, CorsOptions::default(), Location::default_location());
    oc_server::router::build(state)
}

#[tokio::test]
async fn auth_requires_credentials() {
    let router = authed_router();

    let response = send(&router, request(Method::GET, "/api/health")).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers()["www-authenticate"],
        r#"Basic realm="Secure Area""#
    );
    let body = json_body(response).await;
    assert_eq!(body["_tag"], "UnauthorizedError");
    assert_eq!(body["message"], "Authentication required");

    let authorized = Request::builder()
        .method(Method::GET)
        .uri("/api/health")
        .header(header::AUTHORIZATION, "Basic b3BlbmNvZGU6c2VjcmV0")
        .body(Body::empty())
        .unwrap();
    let response = send(&router, authorized).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn credential_endpoints_update_and_remove_durable_rows() {
    let database = Arc::new(Database::open_memory().expect("database"));
    database
        .insert(
            "credential",
            &CredentialRow {
                id: "cred_test".into(),
                integration_id: Some("provider".into()),
                label: "old".into(),
                value: "{\"key\":\"secret\"}".into(),
                connector_id: None,
                method_id: None,
                active: Some(1),
                time_created: 1,
                time_updated: 1,
            },
            &[],
        )
        .expect("insert credential");
    let router = durable_router(database.clone());

    let update = Request::builder()
        .method(Method::PATCH)
        .uri("/api/credential/cred_test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"label":"new","value":{"key":"rotated"}}"#))
        .unwrap();
    assert_eq!(send(&router, update).await.status(), StatusCode::NO_CONTENT);

    let row: CredentialRow = database
        .get_by("credential", "id", &SqlValue::Text("cred_test".into()), &[])
        .expect("read credential")
        .expect("credential exists");
    assert_eq!(row.label, "new");
    assert_eq!(row.value, r#"{"key":"rotated"}"#);

    let remove = request(Method::DELETE, "/api/credential/cred_test");
    assert_eq!(send(&router, remove).await.status(), StatusCode::NO_CONTENT);
    let removed: Option<CredentialRow> = database
        .get_by("credential", "id", &SqlValue::Text("cred_test".into()), &[])
        .expect("read removed credential");
    assert!(removed.is_none());
}

#[tokio::test]
async fn integration_key_connection_replaces_durable_credential() {
    let database = Arc::new(Database::open_memory().expect("database"));
    let router = durable_router(database.clone());
    let connect = Request::builder()
        .method(Method::POST)
        .uri("/api/integration/openai/connect/key")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"key":"sk-test","label":"primary"}"#))
        .unwrap();
    assert_eq!(
        send(&router, connect).await.status(),
        StatusCode::NO_CONTENT
    );

    let rows: Vec<CredentialRow> = database.list("credential", &[]).expect("list credentials");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].integration_id.as_deref(), Some("openai"));
    assert_eq!(rows[0].label, "primary");
    assert!(rows[0].value.contains("sk-test"));
}

#[tokio::test]
async fn auth_token_query_bypasses() {
    let router = authed_router();
    let response = send(
        &router,
        request(Method::GET, "/api/health?auth_token=b3BlbmNvZGU6c2VjcmV0"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn provider_auth_methods_authorize_and_callback_persist_hook_credentials() {
    let _home_lock = TEST_HOME_LOCK.lock().unwrap();
    let test_home = std::env::temp_dir().join(format!(
        "opencode-rs-provider-auth-{}-{}",
        std::process::id(),
        oc_server::event::event_id()
    ));
    std::fs::create_dir_all(&test_home).unwrap();
    let previous_home = std::env::var_os("OPENCODE_TEST_HOME");
    std::env::set_var("OPENCODE_TEST_HOME", &test_home);

    let router = provider_auth_router();
    let methods = json_body(send(&router, request(Method::GET, "/provider/auth")).await).await;
    assert_eq!(methods["test-provider"][0]["type"], "oauth");
    assert_eq!(methods["test-provider"][0]["label"], "Test OAuth");

    let authorize = Request::builder()
        .method(Method::POST)
        .uri("/provider/test-provider/oauth/authorize")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"method":0}"#))
        .unwrap();
    let authorization = json_body(send(&router, authorize).await).await;
    assert_eq!(authorization["url"], "https://auth.example.test/authorize");
    assert_eq!(authorization["method"], "code");

    let failed_callback = Request::builder()
        .method(Method::POST)
        .uri("/provider/test-provider/oauth/callback")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"method":0,"code":"fail"}"#))
        .unwrap();
    let response = send(&router, failed_callback).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["name"], "ProviderAuthError");
    assert!(!test_home.join("data/auth.json").exists());

    let callback = Request::builder()
        .method(Method::POST)
        .uri("/provider/test-provider/oauth/callback")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"method":0,"code":"ok"}"#))
        .unwrap();
    assert_eq!(send(&router, callback).await.status(), StatusCode::OK);
    let stored: Value =
        serde_json::from_slice(&std::fs::read(test_home.join("data/auth.json")).unwrap()).unwrap();
    assert_eq!(stored["test-provider"]["type"], "oauth");
    assert_eq!(stored["test-provider"]["access"], "access-from-hook");

    let second_callback = Request::builder()
        .method(Method::POST)
        .uri("/provider/test-provider/oauth/callback")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"method":0,"code":"ok"}"#))
        .unwrap();
    let response = send(&router, second_callback).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["name"], "ProviderAuthError");

    if let Some(previous_home) = previous_home {
        std::env::set_var("OPENCODE_TEST_HOME", previous_home);
    } else {
        std::env::remove_var("OPENCODE_TEST_HOME");
    }
    std::fs::remove_dir_all(test_home).unwrap();
}

#[tokio::test]
async fn provider_auth_callback_without_authorization_is_an_explicit_error() {
    let response = send(
        &router(),
        Request::builder()
            .method(Method::POST)
            .uri("/provider/unsupported/oauth/callback")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"method":0,"code":"ignored"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["name"], "ProviderAuthError");
    assert_eq!(body["data"]["providerID"], "unsupported");
    assert!(body["data"]["message"]
        .as_str()
        .unwrap()
        .contains("OauthMissing"));
}

#[tokio::test]
async fn integration_oauth_attempt_uses_provider_hook_and_tracks_lifecycle() {
    let _home_lock = TEST_HOME_LOCK.lock().unwrap();
    let test_home = std::env::temp_dir().join(format!(
        "opencode-rs-integration-oauth-{}-{}",
        std::process::id(),
        oc_server::event::event_id()
    ));
    std::fs::create_dir_all(&test_home).unwrap();
    let previous_home = std::env::var_os("OPENCODE_TEST_HOME");
    std::env::set_var("OPENCODE_TEST_HOME", &test_home);

    let router = provider_auth_router();
    let connect = Request::builder()
        .method(Method::POST)
        .uri("/api/integration/test-provider/connect/oauth")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"methodID":"oauth-0","inputs":{}}"#))
        .unwrap();
    let authorization = json_body(send(&router, connect).await).await;
    let attempt_id = authorization["data"]["attemptID"]
        .as_str()
        .expect("attempt id")
        .to_string();
    assert_eq!(authorization["data"]["mode"], "code");
    assert_eq!(
        authorization["data"]["url"],
        "https://auth.example.test/authorize"
    );

    let status = send(
        &router,
        request(
            Method::GET,
            &format!("/api/integration/attempt/{attempt_id}"),
        ),
    )
    .await;
    assert_eq!(json_body(status).await["data"]["status"], "pending");

    let complete = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/integration/attempt/{attempt_id}/complete"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"code":"ok"}"#))
        .unwrap();
    assert_eq!(
        send(&router, complete).await.status(),
        StatusCode::NO_CONTENT
    );

    let status = send(
        &router,
        request(
            Method::GET,
            &format!("/api/integration/attempt/{attempt_id}"),
        ),
    )
    .await;
    assert_eq!(json_body(status).await["data"]["status"], "complete");
    let auth: Value = serde_json::from_slice(
        &std::fs::read(test_home.join("data/auth.json")).expect("persisted OAuth credential"),
    )
    .unwrap();
    assert_eq!(auth["test-provider"]["access"], "access-from-hook");

    let cancel_connect = Request::builder()
        .method(Method::POST)
        .uri("/api/integration/test-provider/connect/oauth")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"methodID":"oauth-0","inputs":{}}"#))
        .unwrap();
    let cancel_attempt = json_body(send(&router, cancel_connect).await).await;
    let cancel_id = cancel_attempt["data"]["attemptID"].as_str().unwrap();
    assert_eq!(
        send(
            &router,
            request(
                Method::DELETE,
                &format!("/api/integration/attempt/{cancel_id}")
            ),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &router,
            request(
                Method::GET,
                &format!("/api/integration/attempt/{cancel_id}")
            ),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    if let Some(previous_home) = previous_home {
        std::env::set_var("OPENCODE_TEST_HOME", previous_home);
    } else {
        std::env::remove_var("OPENCODE_TEST_HOME");
    }
    std::fs::remove_dir_all(test_home).unwrap();
}

/// Read the first complete SSE frame (`...\n\n`) from a streaming body.
async fn read_sse_first_frame(body: Body) -> String {
    let mut stream = body.into_data_stream();
    let mut buffer = String::new();
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("SSE stream timeout")
            .expect("SSE stream ended")
            .expect("SSE body error");
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        if let Some(end) = buffer.find("\n\n") {
            return buffer[..end].to_string();
        }
    }
}
