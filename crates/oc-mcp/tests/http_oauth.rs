//! Integration tests for the Streamable HTTP transport and the OAuth flow,
//! against minimal Python servers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use url::Url;

use oc_mcp::auth::McpAuth;
use oc_mcp::index::Mcp;
use oc_mcp::oauth::AuthOptions;
use oc_mcp::oauth_provider::{
    McpOAuthCallbacks, McpOAuthConfig, McpOAuthProvider, OAuthClientProvider,
};
use oc_mcp::transport::http::StreamableHTTPClientTransport;
use oc_mcp::types::{ClientCapabilities, Implementation};

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oc-mcp-http-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Minimal Streamable HTTP MCP server: POST JSON-RPC, GET keeps an SSE stream
/// open with a session id.
const HTTP_SERVER: &str = r#"
import json, time, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("mcp-session-id", "sess-1")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        try:
            while True:
                time.sleep(3600)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        msg = json.loads(body)
        method = msg.get("method")
        if method == "initialize":
            result = {
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "http-server", "version": "1.0.0"},
            }
        elif method == "tools/list":
            result = {"tools": [
                {"name": "http_tool", "description": "A remote tool", "inputSchema": {"type": "object", "properties": {"x": {"type": "string"}}}}
            ]}
        elif method == "tools/call":
            result = {"content": [{"type": "text", "text": "remote-result"}]}
        else:
            result = {}
        resp = json.dumps({"jsonrpc": "2.0", "id": msg.get("id"), "result": result}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("mcp-session-id", "sess-1")
        self.send_header("Content-Length", str(len(resp)))
        self.end_headers()
        self.wfile.write(resp)

port = int(sys.argv[1])
ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#;

fn spawn_python(dir: &Path, script: &str, port: u16) -> tokio::process::Child {
    let path = dir.join("server.py");
    std::fs::write(&path, script).unwrap();
    tokio::process::Command::new("python3")
        .arg("-u")
        .arg(&path)
        .arg(port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

async fn wait_until_ready(url: &Url) {
    for _ in 0..100 {
        if let Ok(_response) = oc_mcp::oauth::http_client()
            .get(url.clone())
            .header("Accept", "text/event-stream")
            .send()
            .await
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never became ready");
}

#[tokio::test]
async fn streamable_http_transport_roundtrip() {
    let dir = temp_dir();
    let port = free_port().await;
    let url = Url::parse(&format!("http://127.0.0.1:{port}/mcp")).unwrap();
    let mut child = spawn_python(&dir, HTTP_SERVER, port);
    wait_until_ready(&url).await;

    let transport = Arc::new(StreamableHTTPClientTransport::new(url, None, None));
    let client = oc_mcp::client::Client::connect(
        transport,
        Implementation {
            name: "opencode".into(),
            version: "0.1.0".into(),
        },
        ClientCapabilities {
            roots: Some(json!({})),
            sampling: None,
            experimental: None,
        },
        10_000,
    )
    .await
    .unwrap();

    assert_eq!(client.get_server_info().await.unwrap().name, "http-server");
    let tools = client.list_tools(None, 10_000).await.unwrap();
    assert_eq!(tools.tools[0].name, "http_tool");
    let result = client
        .call_tool("http_tool", json!({ "x": "1" }), 10_000)
        .await
        .unwrap();
    assert_eq!(result.content[0].text.as_deref(), Some("remote-result"));

    client.close().await.unwrap();
    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// OAuth server: discovery, dynamic client registration, token endpoint, and
/// an MCP endpoint that 401s until a Bearer token is presented.
const OAUTH_SERVER: &str = r#"
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
BASE = "http://127.0.0.1:%d" % PORT

def protected(self):
    auth = self.headers.get("Authorization", "")
    return auth == "Bearer access-123"

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def _json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _unauthorized(self):
        self.send_response(401)
        self.send_header("WWW-Authenticate", 'Bearer resource="' + BASE + '/.well-known/oauth-protected-resource", scope="mcp"')
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path == "/.well-known/oauth-authorization-server":
            self._json(200, {
                "issuer": BASE,
                "authorization_endpoint": BASE + "/authorize",
                "token_endpoint": BASE + "/token",
                "registration_endpoint": BASE + "/register",
                "scopes_supported": ["mcp", "offline_access"],
                "code_challenge_methods_supported": ["S256"],
            })
        elif self.path == "/.well-known/oauth-protected-resource":
            self._json(200, {"scopes_supported": ["mcp"]})
        elif self.path == "/mcp":
            if not protected(self):
                self._unauthorized()
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("mcp-session-id", "sess-1")
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()
            try:
                while True:
                    time.sleep(3600)
            except (BrokenPipeError, ConnectionResetError):
                pass
        else:
            self._json(404, {"error": "not_found"})

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        if self.path == "/register":
            self._json(201, {"client_id": "registered-client", "client_secret": "reg-secret"})
        elif self.path == "/token":
            self._json(200, {
                "access_token": "access-123",
                "token_type": "Bearer",
                "refresh_token": "refresh-456",
                "expires_in": 3600,
                "scope": "mcp",
            })
        elif self.path == "/mcp":
            msg = json.loads(body)
            if not protected(self):
                self._unauthorized()
                return
            if msg.get("method") == "initialize":
                self._json(200, {"jsonrpc": "2.0", "id": msg.get("id"), "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "oauth-server", "version": "1.0.0"},
                }})
            elif msg.get("method") == "tools/list":
                self._json(200, {"jsonrpc": "2.0", "id": msg.get("id"), "result": {"tools": []}})
            else:
                self._json(200, {"jsonrpc": "2.0", "id": msg.get("id"), "result": {}})
        else:
            self._json(404, {"error": "not_found"})

ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
"#;

#[tokio::test]
async fn oauth_flow_discovers_registers_and_exchanges() {
    let dir = temp_dir();
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let server_url = Url::parse(&format!("{base}/mcp")).unwrap();
    let mut child = spawn_python(&dir, OAUTH_SERVER, port);
    wait_until_ready(&server_url).await;

    let auth = Arc::new(McpAuth::new(dir.join("mcp-auth.json")));
    let captured = Arc::new(tokio::sync::Mutex::new(None::<Url>));
    let captured_clone = captured.clone();
    let provider = McpOAuthProvider::new(
        "oauth-server",
        server_url.to_string(),
        McpOAuthConfig {
            scope: Some("mcp".into()),
            ..Default::default()
        },
        McpOAuthCallbacks {
            on_redirect: Arc::new(move |url: &Url| {
                let captured = captured_clone.clone();
                Box::pin(async move {
                    *captured.lock().await = Some(url.clone());
                    Ok(())
                })
            }),
        },
        Arc::clone(&auth),
    );

    let auth_client = oc_mcp::oauth::AuthClient::new();

    // Discovery + DCR + authorization redirect (interactive).
    let outcome = auth_client
        .auth(
            &provider,
            &AuthOptions {
                server_url: server_url.clone(),
                authorization_code: None,
                scope: Some("mcp".into()),
                resource_metadata_url: None,
            },
        )
        .await;
    assert!(matches!(outcome, Err(oc_mcp::Error::Unauthorized { .. })));

    // The authorization URL was handed to the redirect callback.
    let authorization_url = captured.lock().await.clone().unwrap();
    let query = authorization_url.query().unwrap();
    assert!(query.contains("response_type=code"));
    assert!(query.contains("client_id=registered-client"));
    assert!(query.contains("code_challenge_method=S256"));
    assert!(query.contains("state="));
    assert!(query.contains("resource="));

    // Stored client registration is persisted.
    let entry = auth.get("oauth-server").await.unwrap().unwrap();
    assert_eq!(
        entry.client_info.as_ref().unwrap().client_id,
        "registered-client"
    );

    // Complete the flow with an authorization code.
    let tokens = auth_client
        .finish_with_code(&provider, &server_url, "the-code")
        .await
        .unwrap();
    assert_eq!(tokens.tokens.access_token, "access-123");

    // Tokens were persisted with an expiry.
    let entry = auth.get("oauth-server").await.unwrap().unwrap();
    let stored = entry.tokens.unwrap();
    assert_eq!(stored.access_token, "access-123");
    assert_eq!(stored.refresh_token.as_deref(), Some("refresh-456"));
    assert!(stored.expires_at.unwrap() > 1_700_000_000.0);

    // The stored tokens surface through the provider.
    let tokens = provider.tokens().await.unwrap().unwrap();
    assert_eq!(tokens.access_token, "access-123");

    // Connect a full client through the OAuth-protected endpoint: the 401
    // triggers a token exchange and the retried request succeeds.
    let transport = Arc::new(StreamableHTTPClientTransport::new(
        server_url.clone(),
        None,
        Some(Arc::new(provider)),
    ));
    let client = oc_mcp::client::Client::connect(
        transport,
        Implementation {
            name: "opencode".into(),
            version: "0.1.0".into(),
        },
        ClientCapabilities {
            roots: Some(json!({})),
            sampling: None,
            experimental: None,
        },
        10_000,
    )
    .await
    .unwrap();
    assert_eq!(client.get_server_info().await.unwrap().name, "oauth-server");

    client.close().await.unwrap();
    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn mcp_service_start_auth_and_finish_auth() {
    let dir = temp_dir();
    let port = free_port().await;
    let server_url = format!("http://127.0.0.1:{port}/mcp");
    let mut child = spawn_python(&dir, OAUTH_SERVER, port);
    wait_until_ready(&Url::parse(&server_url).unwrap()).await;

    let mut config = indexmap::IndexMap::new();
    config.insert(
        "remote-1".to_string(),
        oc_mcp::config::Info::Remote(oc_mcp::config::Remote {
            url: server_url.clone(),
            enabled: Some(true),
            headers: None,
            oauth: Some(oc_mcp::config::OAuth::Config(oc_mcp::config::OAuthConfig {
                client_id: None,
                client_secret: None,
                scope: Some("mcp".into()),
                callback_port: None,
                redirect_uri: None,
            })),
            timeout: Some(10_000),
        }),
    );

    let auth = Arc::new(McpAuth::new(dir.join("mcp-auth.json")));
    let mcp = Mcp::with_options(
        config,
        dir.clone(),
        oc_mcp::index::McpOptions {
            auth: Some(auth),
            default_timeout: Some(10_000),
            events: None,
        },
    );
    mcp.init().await;

    // init() hits the 401 and should report needs_auth.
    let status = mcp.status().await;
    assert_eq!(status["remote-1"], oc_mcp::index::Status::NeedsAuth);

    // start_auth returns the authorization URL.
    let started = mcp.start_auth("remote-1").await.unwrap();
    assert!(!started.authorization_url.is_empty());
    assert!(!started.oauth_state.is_empty());

    // finish_auth exchanges the code, persists tokens, and connects.
    let status = mcp.finish_auth("remote-1", "the-code").await.unwrap();
    assert_eq!(status, oc_mcp::index::Status::Connected);

    assert!(mcp.has_stored_tokens("remote-1").await.unwrap());
    assert_eq!(
        mcp.get_auth_status("remote-1").await.unwrap(),
        oc_mcp::index::AuthStatus::Authenticated
    );
    assert_eq!(
        mcp.status().await["remote-1"],
        oc_mcp::index::Status::Connected
    );

    // remove_auth clears the stored tokens.
    mcp.remove_auth("remote-1").await.unwrap();
    assert!(!mcp.has_stored_tokens("remote-1").await.unwrap());

    mcp.close_all().await;
    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
}
