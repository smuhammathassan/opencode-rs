//! End-to-end handler tests over the axum router.
//!
//! Golden assertions mirror the reference handler output (reference/packages/server/
//! src/handlers/* and reference/packages/opencode/src/server/routes/instance/httpapi/
//! handlers/*). Requests are dispatched with `tower::ServiceExt::oneshot` so no socket
//! is needed.

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use futures::StreamExt;
use oc_server::auth::AuthConfig;
use oc_server::cors::CorsOptions;
use oc_server::location::Location;
use oc_server::state::AppState;
use tower::ServiceExt;

fn router() -> axum::Router {
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
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
async fn config_get_returns_config() {
    let router = router();
    let response = send(&router, request(Method::GET, "/config")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body.is_object());
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
async fn auth_token_query_bypasses() {
    let router = authed_router();
    let response = send(
        &router,
        request(Method::GET, "/api/health?auth_token=b3BlbmNvZGU6c2VjcmV0"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
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
