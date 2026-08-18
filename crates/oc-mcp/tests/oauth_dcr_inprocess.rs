//! In-process OAuth dynamic client registration tests using an embedded axum
//! mock OAuth server. Drives the full non-browser state machine: discovery →
//! dynamic registration → authorize URL construction → token exchange →
//! refresh → persisted state, plus error paths. (The browser step is headless:
//! the flow stops at the authorization URL.)

use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use url::Url;

use oc_mcp::auth::McpAuth;
use oc_mcp::oauth::{AuthClient, AuthOptions};
use oc_mcp::oauth_provider::{McpOAuthCallbacks, McpOAuthConfig, McpOAuthProvider};

#[derive(Clone)]
struct ServerState {
    register_calls: Arc<Mutex<usize>>,
    token_calls: Arc<Mutex<Vec<String>>>,
    refresh_calls: Arc<Mutex<usize>>,
    registration_enabled: Arc<Mutex<bool>>,
    registration_returns_valid: Arc<Mutex<bool>>,
    token_ok: Arc<Mutex<bool>>,
    base: Arc<Mutex<String>>,
}

async fn well_known(State(state): State<ServerState>) -> Json<Value> {
    let base = state.base.lock().unwrap().clone();
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
        "scopes_supported": ["mcp", "offline_access"],
        "code_challenge_methods_supported": ["S256"],
    }))
}

async fn register(State(state): State<ServerState>) -> (axum::http::StatusCode, Json<Value>) {
    *state.register_calls.lock().unwrap() += 1;
    if !*state.registration_enabled.lock().unwrap() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"error": "registration_not_supported"})),
        );
    }
    if !*state.registration_returns_valid.lock().unwrap() {
        // RFC 7591 requires client_id; an empty one is an error.
        return (
            axum::http::StatusCode::CREATED,
            Json(json!({"client_id": "", "client_secret": "reg-secret"})),
        );
    }
    (
        axum::http::StatusCode::CREATED,
        Json(json!({
            "client_id": "registered-client",
            "client_secret": "reg-secret",
            "client_secret_expires_at": 0,
        })),
    )
}

async fn token(
    State(state): State<ServerState>,
    form: axum::extract::Form<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let grant_type = form.get("grant_type").and_then(Value::as_str).unwrap_or("");
    state
        .token_calls
        .lock()
        .unwrap()
        .push(grant_type.to_string());
    if !*state.token_ok.lock().unwrap() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant"})),
        );
    }
    if grant_type == "refresh_token" {
        *state.refresh_calls.lock().unwrap() += 1;
        return (
            axum::http::StatusCode::OK,
            Json(json!({
                "access_token": "access-refreshed",
                "token_type": "Bearer",
                "refresh_token": "refresh-456",
                "expires_in": 3600,
                "scope": "mcp",
            })),
        );
    }
    // authorization_code grant
    (
        axum::http::StatusCode::OK,
        Json(json!({
            "access_token": "access-123",
            "token_type": "Bearer",
            "refresh_token": "refresh-456",
            "expires_in": 3600,
            "scope": "mcp",
        })),
    )
}

async fn authorize() -> Json<Value> {
    Json(json!({"code": "the-code"}))
}

async fn protected_resource_metadata(State(state): State<ServerState>) -> Json<Value> {
    let base = state.base.lock().unwrap().clone();
    Json(json!({"resource": base, "scopes_supported": ["mcp"]}))
}

fn build_router(state: ServerState) -> Router {
    Router::new()
        .route("/.well-known/oauth-authorization-server", get(well_known))
        .route("/authorize", get(authorize))
        .route("/register", post(register))
        .route("/token", post(token))
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .with_state(state)
}

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("oc-mcp-oauth-inproc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Start the axum server on an ephemeral port and return its base URL.
async fn start_server(state: ServerState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    *state.base.lock().unwrap() = base.clone();
    tokio::spawn(async move {
        axum::serve(listener, build_router(state)).await.unwrap();
    });
    base
}

fn provider(auth: Arc<McpAuth>, server_url: &str) -> (McpOAuthProvider, Arc<Mutex<Option<Url>>>) {
    let captured = Arc::new(Mutex::new(None::<Url>));
    let captured_clone = Arc::clone(&captured);
    let provider = McpOAuthProvider::new(
        "oauth-server",
        server_url,
        McpOAuthConfig {
            scope: Some("mcp".into()),
            ..Default::default()
        },
        McpOAuthCallbacks {
            on_redirect: Arc::new(move |url: &Url| {
                let captured = captured_clone.clone();
                Box::pin(async move {
                    *captured.lock().unwrap() = Some(url.clone());
                    Ok(())
                })
            }),
        },
        Arc::clone(&auth),
    );
    (provider, captured)
}

#[tokio::test(flavor = "multi_thread")]
async fn full_state_machine_discovers_registers_exchanges_and_persists() {
    let dir = temp_dir();
    let state = ServerState {
        register_calls: Arc::new(Mutex::new(0)),
        token_calls: Arc::new(Mutex::new(Vec::new())),
        refresh_calls: Arc::new(Mutex::new(0)),
        registration_enabled: Arc::new(Mutex::new(true)),
        registration_returns_valid: Arc::new(Mutex::new(true)),
        token_ok: Arc::new(Mutex::new(true)),
        base: Arc::new(Mutex::new(String::new())),
    };
    let base = start_server(state.clone()).await;
    let server_url = format!("{base}/mcp");

    let auth = Arc::new(McpAuth::new(dir.join("mcp-auth.json")));
    let (provider, captured) = provider(auth.clone(), &server_url);
    let auth_client = AuthClient::new();

    // Discovery + DCR + interactive authorize redirect (browser step is a URL).
    let outcome = auth_client
        .auth(
            &provider,
            &AuthOptions {
                server_url: Url::parse(&server_url).unwrap(),
                authorization_code: None,
                scope: Some("mcp".into()),
                resource_metadata_url: Some(
                    Url::parse(&format!("{base}/.well-known/oauth-protected-resource")).unwrap(),
                ),
            },
        )
        .await;
    assert!(matches!(outcome, Err(oc_mcp::Error::Unauthorized { .. })));
    assert_eq!(*state.register_calls.lock().unwrap(), 1);

    // The authorization URL carries the registered client id and PKCE.
    let authorization_url = captured.lock().unwrap().clone().unwrap();
    let query = authorization_url.query().unwrap();
    assert!(query.contains("response_type=code"));
    assert!(query.contains("client_id=registered-client"));
    assert!(query.contains("code_challenge_method=S256"));
    assert!(query.contains("state="));
    assert!(query.contains("resource="));

    // Registration was persisted.
    let entry = auth.get("oauth-server").await.unwrap().unwrap();
    assert_eq!(
        entry.client_info.as_ref().unwrap().client_id,
        "registered-client"
    );

    // Complete the interactive exchange.
    let tokens = auth_client
        .finish_with_code(&provider, &Url::parse(&server_url).unwrap(), "the-code")
        .await
        .unwrap();
    assert_eq!(tokens.tokens.access_token, "access-123");
    assert!(auth
        .get("oauth-server")
        .await
        .unwrap()
        .unwrap()
        .tokens
        .is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_tokens_are_refreshed_via_refresh_token_grant() {
    let dir = temp_dir();
    let state = ServerState {
        register_calls: Arc::new(Mutex::new(0)),
        token_calls: Arc::new(Mutex::new(Vec::new())),
        refresh_calls: Arc::new(Mutex::new(0)),
        registration_enabled: Arc::new(Mutex::new(true)),
        registration_returns_valid: Arc::new(Mutex::new(true)),
        token_ok: Arc::new(Mutex::new(true)),
        base: Arc::new(Mutex::new(String::new())),
    };
    let base = start_server(state.clone()).await;
    let server_url = format!("{base}/mcp");
    let resource_url = Url::parse(&format!("{base}/.well-known/oauth-protected-resource")).unwrap();

    let auth = Arc::new(McpAuth::new(dir.join("mcp-auth.json")));
    // Seed stored client info + an expired access token with a refresh token.
    auth.set(
        "oauth-server",
        oc_mcp::auth::Entry {
            client_info: Some(oc_mcp::auth::ClientInfo {
                client_id: "registered-client".into(),
                client_secret: Some("reg-secret".into()),
                client_secret_expires_at: Some(0.0),
                ..Default::default()
            }),
            tokens: Some(oc_mcp::auth::Tokens {
                access_token: "old-access".into(),
                refresh_token: Some("refresh-456".into()),
                expires_at: Some(1.0), // already expired
                scope: Some("mcp".into()),
            }),
            ..Default::default()
        },
        Some(&server_url),
    )
    .await
    .unwrap();

    let (provider, _captured) = provider(auth.clone(), &server_url);
    let auth_client = AuthClient::new();

    // auth() with stored credentials and no code should refresh the expired
    // token rather than starting a new authorization.
    let outcome = auth_client
        .auth(
            &provider,
            &AuthOptions {
                server_url: Url::parse(&server_url).unwrap(),
                authorization_code: None,
                scope: Some("mcp".into()),
                resource_metadata_url: Some(resource_url),
            },
        )
        .await
        .unwrap();
    assert_eq!(outcome.tokens.access_token, "access-refreshed");

    let grants = state.token_calls.lock().unwrap();
    assert!(grants.iter().any(|grant| grant == "refresh_token"));
    assert!(!grants.iter().any(|grant| grant == "authorization_code"));
    assert_eq!(
        *state.register_calls.lock().unwrap(),
        0,
        "no DCR on refresh"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_client_id_in_registration_response_is_an_error() {
    let dir = temp_dir();
    let state = ServerState {
        register_calls: Arc::new(Mutex::new(0)),
        token_calls: Arc::new(Mutex::new(Vec::new())),
        refresh_calls: Arc::new(Mutex::new(0)),
        registration_enabled: Arc::new(Mutex::new(true)),
        registration_returns_valid: Arc::new(Mutex::new(false)),
        token_ok: Arc::new(Mutex::new(true)),
        base: Arc::new(Mutex::new(String::new())),
    };
    let base = start_server(state.clone()).await;
    let server_url = format!("{base}/mcp");

    let auth = Arc::new(McpAuth::new(dir.join("mcp-auth.json")));
    let (provider, _captured) = provider(auth.clone(), &server_url);
    let auth_client = AuthClient::new();

    let outcome = auth_client
        .auth(
            &provider,
            &AuthOptions {
                server_url: Url::parse(&server_url).unwrap(),
                authorization_code: None,
                scope: Some("mcp".into()),
                resource_metadata_url: None,
            },
        )
        .await;
    let error = match outcome {
        Err(error) => error,
        Ok(_) => panic!("expected registration to fail"),
    };
    let message = error.to_string();
    assert!(
        message.contains("missing client_id"),
        "expected missing client_id error, got: {message}"
    );
    assert!(matches!(error, oc_mcp::Error::Unauthorized { .. }));
    // No client info should be persisted since registration was invalid.
    let entry = auth.get("oauth-server").await.unwrap();
    assert!(entry.is_none() || entry.unwrap().client_info.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn token_endpoint_failure_is_reported_as_oauth_error() {
    let dir = temp_dir();
    let state = ServerState {
        register_calls: Arc::new(Mutex::new(0)),
        token_calls: Arc::new(Mutex::new(Vec::new())),
        refresh_calls: Arc::new(Mutex::new(0)),
        registration_enabled: Arc::new(Mutex::new(true)),
        registration_returns_valid: Arc::new(Mutex::new(true)),
        token_ok: Arc::new(Mutex::new(false)),
        base: Arc::new(Mutex::new(String::new())),
    };
    let base = start_server(state.clone()).await;
    let server_url = format!("{base}/mcp");

    let auth = Arc::new(McpAuth::new(dir.join("mcp-auth.json")));
    let (provider, _captured) = provider(auth.clone(), &server_url);
    let auth_client = AuthClient::new();

    // Run the interactive step first so a code verifier is saved and client
    // registration completes; the authorization step never hits the token
    // endpoint so it succeeds even with token_ok=false.
    let _outcome = auth_client
        .auth(
            &provider,
            &AuthOptions {
                server_url: Url::parse(&server_url).unwrap(),
                authorization_code: None,
                scope: Some("mcp".into()),
                resource_metadata_url: None,
            },
        )
        .await;

    // The code exchange now fails at the token endpoint.
    let result = auth_client
        .finish_with_code(&provider, &Url::parse(&server_url).unwrap(), "the-code")
        .await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("expected token exchange to fail"),
    };
    assert!(matches!(
        error,
        oc_mcp::Error::OAuth(message) if message.contains("Token request failed")
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_registration_endpoint_yields_needs_registration() {
    let dir = temp_dir();
    // A server whose well-known metadata has no registration_endpoint.
    let state = ServerState {
        register_calls: Arc::new(Mutex::new(0)),
        token_calls: Arc::new(Mutex::new(Vec::new())),
        refresh_calls: Arc::new(Mutex::new(0)),
        registration_enabled: Arc::new(Mutex::new(false)),
        registration_returns_valid: Arc::new(Mutex::new(true)),
        token_ok: Arc::new(Mutex::new(true)),
        base: Arc::new(Mutex::new(String::new())),
    };
    // We start the normal server but disable registration; discovery still
    // advertises the endpoint but registration returns 404, exercising the
    // error path in `ensure_client_information`.
    let base = start_server(state.clone()).await;
    let server_url = format!("{base}/mcp");

    let auth = Arc::new(McpAuth::new(dir.join("mcp-auth.json")));
    let (provider, _captured) = provider(auth.clone(), &server_url);
    let auth_client = AuthClient::new();

    let outcome = auth_client
        .auth(
            &provider,
            &AuthOptions {
                server_url: Url::parse(&server_url).unwrap(),
                authorization_code: None,
                scope: Some("mcp".into()),
                resource_metadata_url: None,
            },
        )
        .await;
    assert!(matches!(outcome, Err(oc_mcp::Error::Unauthorized { .. })));
    let _ = std::fs::remove_dir_all(&dir);
}
