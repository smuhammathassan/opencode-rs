//! Focused tests for the embedded browser entrypoint.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use oc_server::auth::AuthConfig;
use oc_server::cors::CorsOptions;
use oc_server::location::Location;
use oc_server::state::AppState;
use tower::ServiceExt;

fn router() -> axum::Router {
    oc_server::router::build(AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    ))
}

async fn get(router: &axum::Router, path: &str) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn root_serves_embedded_browser_client() {
    let response = get(&router(), "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    let body = axum::body::to_bytes(response.into_body(), 128 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Browser workspace"));
    assert!(html.contains("/assets/app.js"));
}

#[tokio::test]
async fn embedded_assets_are_served_with_their_content_types() {
    let router = router();
    let script = get(&router, "/assets/app.js").await;
    assert_eq!(script.status(), StatusCode::OK);
    assert_eq!(
        script.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    let script_body = axum::body::to_bytes(script.into_body(), 256 * 1024)
        .await
        .unwrap();
    let script = String::from_utf8(script_body.to_vec()).unwrap();
    assert!(script.contains("/global/health"));
    assert!(script.contains("/session/"));

    let style = get(&router, "/assets/app.css").await;
    assert_eq!(style.status(), StatusCode::OK);
    assert_eq!(
        style.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
}

#[tokio::test]
async fn unknown_api_paths_remain_not_found() {
    let response = get(&router(), "/api/does-not-exist").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
