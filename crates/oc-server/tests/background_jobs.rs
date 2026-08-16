use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use oc_core::background_job::{Run, StartInput};
use oc_server::auth::AuthConfig;
use oc_server::cors::CorsOptions;
use oc_server::location::Location;
use oc_server::state::AppState;
use serde_json::Value;
use tokio::sync::Notify;
use tower::ServiceExt;

fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn experimental_background_routes_list_status_promote_and_cancel() {
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let gate = Arc::new(Notify::new());
    let run_gate = gate.clone();
    let run: Run = Arc::new(move || {
        let gate = run_gate.clone();
        Box::pin(async move {
            gate.notified().await;
            Ok("finished".into())
        })
    });
    state
        .background_jobs
        .start(StartInput {
            id: Some("ses_background_http".into()),
            r#type: "subagent".into(),
            title: Some("HTTP background job".into()),
            metadata: None,
            on_promote: None,
            run,
        })
        .await;

    let router = oc_server::router::build(state);
    let list = router
        .clone()
        .oneshot(request(Method::GET, "/experimental/session/background"))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list = json_body(list).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["status"], "running");

    let promoted = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/experimental/session/ses_background_http/background",
        ))
        .await
        .unwrap();
    assert_eq!(promoted.status(), StatusCode::OK);
    assert_eq!(json_body(promoted).await, Value::Bool(true));

    let status = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/experimental/session/ses_background_http/background",
        ))
        .await
        .unwrap();
    let status = json_body(status).await;
    assert_eq!(status["status"], "running");
    assert_eq!(status["metadata"]["background"], true);

    let cancelled = router
        .clone()
        .oneshot(request(
            Method::DELETE,
            "/experimental/session/ses_background_http/background",
        ))
        .await
        .unwrap();
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_eq!(json_body(cancelled).await["status"], "cancelled");

    let missing = router
        .oneshot(request(
            Method::GET,
            "/experimental/session/unknown/background",
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    gate.notify_one();
}
