use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use oc_server::auth::AuthConfig;
use oc_server::cors::CorsOptions;
use oc_server::location::Location;
use oc_server::state::AppState;
use serde_json::Value;
use tower::ServiceExt;

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

#[tokio::test]
async fn fs_list_returns_sorted_location_relative_entries() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("opencode-rs-fs-list-{suffix}"));
    std::fs::create_dir_all(root.join("a-directory")).expect("create directory entry");
    std::fs::write(root.join("z-file.txt"), "z").expect("create file entry");
    std::fs::write(root.join("a-file.txt"), "a").expect("create file entry");

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let router = oc_server::router::build(state);
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/fs/list")
        .header("x-opencode-directory", root.to_string_lossy().as_ref())
        .body(Body::empty())
        .expect("build request");

    let response = router.oneshot(request).await.expect("dispatch request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(
        body["data"],
        serde_json::json!([
            { "path": "a-directory/", "type": "directory" },
            { "path": "a-file.txt", "type": "file" },
            { "path": "z-file.txt", "type": "file" },
        ])
    );

    std::fs::remove_dir_all(root).expect("remove test directory");
}

#[tokio::test]
async fn fs_list_rejects_missing_directory() {
    let root = std::env::temp_dir().join(format!(
        "opencode-rs-fs-list-missing-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let router = oc_server::router::build(state);
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/fs/list?path=missing")
        .header("x-opencode-directory", root.to_string_lossy().as_ref())
        .body(Body::empty())
        .expect("build request");

    let response = router.oneshot(request).await.expect("dispatch request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn fs_read_rejects_parent_traversal() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("opencode-rs-fs-read-{suffix}"));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("inside.txt"), "inside").expect("create inside file");

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let router = oc_server::router::build(state);
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/fs/read/../outside.txt")
        .header("x-opencode-directory", root.to_string_lossy().as_ref())
        .body(Body::empty())
        .expect("build request");

    let response = router.oneshot(request).await.expect("dispatch request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    std::fs::remove_dir_all(root).expect("remove test directory");
}

#[tokio::test]
async fn legacy_file_routes_remain_scoped_to_active_location() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("opencode-rs-file-scope-{suffix}"));
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).expect("create outside directory");
    std::fs::write(root.join("inside.txt"), "inside").expect("create inside file");
    std::fs::write(outside.join("secret.txt"), "secret").expect("create outside file");

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::with_directory(root.to_string_lossy().as_ref(), None),
    );
    let router = oc_server::router::build(state);
    let encoded_outside =
        url::form_urlencoded::byte_serialize(outside.to_string_lossy().as_bytes())
            .collect::<String>();
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!(
            "/file/content?directory={encoded_outside}&path=secret.txt"
        ))
        .body(Body::empty())
        .expect("build request");

    let response = router.oneshot(request).await.expect("dispatch request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    std::fs::remove_dir_all(root).expect("remove test directory");
}

#[tokio::test]
async fn fs_find_returns_protocol_entries_and_directory_matches() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("opencode-rs-fs-find-{suffix}"));
    std::fs::create_dir_all(root.join("src/nested")).expect("create nested directories");
    std::fs::write(root.join("src/nested/needle.rs"), "needle").expect("create file");

    let state = AppState::new(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
    );
    let router = oc_server::router::build(state);
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/fs/find?query=needle&type=file&limit=1")
        .header("x-opencode-directory", root.to_string_lossy().as_ref())
        .body(Body::empty())
        .expect("build request");

    let response = router.oneshot(request).await.expect("dispatch request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(
        body["data"],
        serde_json::json!([
            { "path": "src/nested/needle.rs", "type": "file" }
        ])
    );

    std::fs::remove_dir_all(root).expect("remove test directory");
}
