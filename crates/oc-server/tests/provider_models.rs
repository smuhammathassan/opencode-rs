//! Focused F079 coverage for the v2 provider/model catalog endpoints.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use oc_server::auth::AuthConfig;
use oc_server::cors::CorsOptions;
use oc_server::location::Location;
use oc_server::state::AppState;
use serde_json::Value;
use std::ffi::OsString;
use std::sync::Mutex;
use tower::ServiceExt;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvRestore {
    key: &'static str,
    previous: Option<OsString>,
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

async fn send(router: &axum::Router, request: Request<Body>) -> Response {
    router.clone().oneshot(request).await.expect("response")
}

async fn json_body(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json body")
}

#[tokio::test]
async fn provider_and_model_catalog_endpoints_use_typed_registry_projection() {
    let _env_lock = ENV_LOCK.lock().expect("environment lock");
    let env_key = "OPENCODE_F079_TEST_KEY";
    let _restore = EnvRestore {
        key: env_key,
        previous: std::env::var_os(env_key),
    };
    std::env::set_var(env_key, "f079-secret");

    let state = AppState::new_with_config(
        AuthConfig::default(),
        CorsOptions::default(),
        Location::default_location(),
        serde_json::json!({
            "provider": {
                "local": {
                    "name": "Local Gateway",
                    "env": [env_key],
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
    assert!(providers["location"]["directory"].is_string());
    let provider = providers["data"]
        .as_array()
        .expect("provider catalog")
        .iter()
        .find(|provider| provider["id"] == "local")
        .expect("custom provider");
    assert_eq!(provider["name"], "Local Gateway");
    assert_eq!(provider["models"]["demo"]["providerID"], "local");
    assert!(
        provider.get("key").is_none(),
        "provider key leaked: {provider}"
    );

    let provider_detail =
        json_body(send(&router, request(Method::GET, "/api/provider/local")).await).await;
    assert_eq!(
        provider_detail["location"]["directory"],
        providers["location"]["directory"]
    );
    assert_eq!(provider_detail["data"]["id"], "local");
    assert!(provider_detail["data"].get("key").is_none());

    let models = json_body(send(&router, request(Method::GET, "/api/model")).await).await;
    assert!(models["location"]["directory"].is_string());
    let model = models["data"]
        .as_array()
        .expect("model catalog")
        .iter()
        .find(|model| model["id"] == "demo" && model["providerID"] == "local")
        .expect("custom model");
    assert_eq!(model["name"], "Demo Model");
    assert_eq!(model["api"]["id"], "demo");

    let missing = send(
        &router,
        request(Method::GET, "/api/provider/does-not-exist"),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
