//! Integration tests against a real (Python) MCP stdio server.
//!
//! The server implements the MCP protocol over newline-delimited JSON: the
//! `initialize` handshake, `notifications/initialized`, server→client
//! `roots/list`, paginated `tools/list`, `tools/call` (including `isError`),
//! `prompts/*` and `resources/*`. Every line the client sends is logged so the
//! wire format can be asserted exactly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use serde_json::json;

use oc_mcp::auth::McpAuth;
use oc_mcp::client::{register_roots_handler, Client};
use oc_mcp::config::{Info, Local};
use oc_mcp::index::{AuthStatus, Mcp, Status};
use oc_mcp::transport::stdio::StdioTransport;
use oc_mcp::types::{ClientCapabilities, Implementation};

const SERVER_SCRIPT: &str = r#"
import sys, json, os

log_path = os.environ["MCP_TEST_LOG"]
log = open(log_path, "w")

def respond(msg_id, result):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "result": result}) + "\n")
    sys.stdout.flush()

def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()

page1 = [
    {"name": "echo", "description": "Echo text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}
]
page2 = [
    {"name": "add", "description": "Add numbers", "inputSchema": {"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}}
]

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    if "method" in msg:
        log.write(line + "\n")
        log.flush()
        method = msg["method"]
        params = msg.get("params", {})
        if method == "initialize":
            respond(msg["id"], {
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {"listChanged": True}, "resources": {}, "prompts": {}},
                "serverInfo": {"name": "test-server", "version": "1.0.0"},
                "instructions": "Use these tools.",
            })
        elif method == "notifications/initialized":
            send({"jsonrpc": "2.0", "id": 100, "method": "roots/list"})
        elif method == "ping":
            respond(msg["id"], {})
        elif method == "tools/list":
            cursor = params.get("cursor")
            if cursor is None:
                respond(msg["id"], {"tools": page1, "nextCursor": "page2"})
            else:
                respond(msg["id"], {"tools": page2})
        elif method == "tools/call":
            name = params["name"]
            args = params.get("arguments", {})
            if name == "fail":
                respond(msg["id"], {"content": [{"type": "text", "text": "boom"}], "isError": True})
            elif name == "echo":
                respond(msg["id"], {"content": [{"type": "text", "text": args.get("text", "")}]})
            else:
                respond(msg["id"], {"content": [{"type": "text", "text": "ok"}]})
        elif method == "prompts/list":
            respond(msg["id"], {"prompts": [{"name": "greet", "arguments": [{"name": "who", "required": True}]}]})
        elif method == "prompts/get":
            respond(msg["id"], {"description": "greet", "messages": [{"role": "user", "content": {"type": "text", "text": "hi"}}]})
        elif method == "resources/list":
            respond(msg["id"], {"resources": [{"uri": "file:///tmp/x", "name": "x"}]})
        elif method == "resources/templates/list":
            respond(msg["id"], {"resourceTemplates": [{"uriTemplate": "file:///{path}", "name": "file"}]})
        elif method == "resources/read":
            respond(msg["id"], {"contents": [{"uri": "file:///tmp/x", "text": "content"}]})
        else:
            respond(msg["id"], {})
    else:
        log.write("CLIENT_RESPONSE " + line + "\n")
        log.flush()
"#;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oc-mcp-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_server(dir: &Path, _log_path: &str) -> String {
    let path = dir.join("test_server.py");
    std::fs::write(&path, SERVER_SCRIPT).unwrap();
    path.to_string_lossy().into_owned()
}

fn client_info() -> Implementation {
    Implementation {
        name: "opencode".into(),
        version: "0.1.0".into(),
    }
}

fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        roots: Some(json!({})),
        sampling: None,
        experimental: None,
    }
}

async fn wait_for_line(log_path: &Path, needle: &str) {
    for _ in 0..200 {
        if let Ok(content) = std::fs::read_to_string(log_path) {
            if content.contains(needle) {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {needle:?} in MCP server log")
}

#[tokio::test]
async fn stdio_client_initialize_list_and_call() {
    let dir = temp_dir();
    let log_path = dir.join("received.jsonl");
    let script = write_server(&dir, &log_path.to_string_lossy());
    let env = vec![(
        "MCP_TEST_LOG".into(),
        log_path.to_string_lossy().into_owned(),
    )];

    let transport = StdioTransport::new(
        "python3".into(),
        vec!["-u".into(), script],
        dir.clone(),
        env,
    );
    let client = Client::spawn(Arc::new(transport), client_info(), client_capabilities())
        .await
        .unwrap();
    register_roots_handler(&client, &dir).await;
    client.initialize(10_000).await.unwrap();

    assert!(client.is_initialized());
    let capabilities = client.get_server_capabilities().await.unwrap();
    assert!(capabilities.has_tools());
    assert!(capabilities.has_resources());
    assert!(capabilities.has_prompts());
    assert_eq!(
        client.get_instructions().await.as_deref(),
        Some("Use these tools.")
    );

    // The server requests roots/list after initialization; the client must
    // answer with the workspace directory.
    wait_for_line(&log_path, "CLIENT_RESPONSE").await;
    let log = std::fs::read_to_string(&log_path).unwrap();
    let client_response = log
        .lines()
        .find(|line| line.starts_with("CLIENT_RESPONSE"))
        .unwrap()
        .to_string();
    let response: serde_json::Value =
        serde_json::from_str(&client_response["CLIENT_RESPONSE ".len()..]).unwrap();
    assert_eq!(response["id"], 100);
    let root_uri = response["result"]["roots"][0]["uri"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .replace('\\', "/");
    let dir_name = dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    assert!(
        root_uri.contains(&dir_name),
        "roots uri should contain the workspace directory; uri={root_uri}, dir_name={dir_name}"
    );

    // Paginated tools/list.
    let first = client.list_tools(None, 10_000).await.unwrap();
    assert_eq!(first.tools.len(), 1);
    assert_eq!(first.tools[0].name, "echo");
    let second = client
        .list_tools(Some("page2".into()), 10_000)
        .await
        .unwrap();
    assert_eq!(second.tools[0].name, "add");

    // tools/call success.
    let result = client
        .call_tool("echo", json!({ "text": "hi" }), 10_000)
        .await
        .unwrap();
    assert_eq!(result.content[0].text.as_deref(), Some("hi"));

    // tools/call with isError.
    let failed = client.call_tool("fail", json!({}), 10_000).await.unwrap();
    assert!(failed.is_error);

    // prompts/resources.
    let prompts = client.list_prompts(None, 10_000).await.unwrap();
    assert_eq!(prompts.prompts[0].name, "greet");
    let prompt = client
        .get_prompt("greet", Some(json!({ "who": "$1" })), 10_000)
        .await
        .unwrap();
    assert_eq!(prompt.description.as_deref(), Some("greet"));
    assert_eq!(prompt.messages[0]["content"]["text"], "hi");
    let resources = client.list_resources(None, 10_000).await.unwrap();
    assert_eq!(resources.resources[0].name, "x");
    let read = client.read_resource("file:///tmp/x", 10_000).await.unwrap();
    assert_eq!(read.contents[0].text.as_deref(), Some("content"));

    client.close().await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn stdio_wire_messages_match_reference() {
    let dir = temp_dir();
    let log_path = dir.join("received.jsonl");
    let script = write_server(&dir, &log_path.to_string_lossy());
    let env = vec![(
        "MCP_TEST_LOG".into(),
        log_path.to_string_lossy().into_owned(),
    )];

    let transport = StdioTransport::new(
        "python3".into(),
        vec!["-u".into(), script],
        dir.clone(),
        env,
    );
    let client = Client::spawn(Arc::new(transport), client_info(), client_capabilities())
        .await
        .unwrap();
    register_roots_handler(&client, &dir).await;
    client.initialize(10_000).await.unwrap();
    wait_for_line(&log_path, "CLIENT_RESPONSE").await;

    let _ = client.list_tools(None, 10_000).await.unwrap();
    let _ = client
        .call_tool("echo", json!({ "text": "hi" }), 10_000)
        .await
        .unwrap();
    client.close().await.unwrap();

    let log = std::fs::read_to_string(&log_path).unwrap();
    let client_lines: Vec<String> = log
        .lines()
        .filter(|line| !line.starts_with("CLIENT_RESPONSE"))
        .map(str::to_string)
        .collect();

    // initialize request (params keys are insertion-ordered because the
    // workspace enables serde_json preserve_order, matching the reference JS
    // object literal order).
    assert_eq!(
        client_lines[0],
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{"roots":{}},"clientInfo":{"name":"opencode","version":"0.1.0"}}}"#
    );
    assert_eq!(
        client_lines[1],
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
    );
    // tools/list without params omits `params`.
    assert_eq!(
        client_lines[2],
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#
    );
    // tools/call carries `_meta.progressToken` (onprogress hook is present).
    assert_eq!(
        client_lines[3],
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"},"_meta":{"progressToken":1}}}"#
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn mcp_service_connects_configured_servers() {
    let dir = temp_dir();
    let log_path = dir.join("service.jsonl");
    let script = write_server(&dir, &log_path.to_string_lossy());

    let mut config = IndexMap::new();
    config.insert(
        "test".to_string(),
        Info::Local(Local {
            command: vec!["python3".into(), "-u".into(), script],
            cwd: None,
            environment: Some(HashMap::from([(
                "MCP_TEST_LOG".to_string(),
                log_path.to_string_lossy().into_owned(),
            )])),
            enabled: Some(true),
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

    let status = mcp.status().await;
    eprintln!("status = {:?}", status);
    assert_eq!(status["test"], Status::Connected);

    // Two paginated tools, keyed by sanitize(client)_sanitize(tool).
    let tools = mcp.tools().await;
    assert!(tools.contains_key("test_echo"));
    assert!(tools.contains_key("test_add"));

    // Instructions include the connected server and its tools.
    let instructions = mcp.instructions().await;
    assert_eq!(instructions.len(), 1);
    assert_eq!(instructions[0].name, "test");
    assert_eq!(instructions[0].tools, vec!["test_echo", "test_add"]);

    // prompts/resources enriched with the client name.
    let prompts = mcp.prompts().await.unwrap();
    assert_eq!(prompts["test:greet"]["name"], "greet");
    assert_eq!(prompts["test:greet"]["client"], "test");
    let resources = mcp.resources(None).await.unwrap();
    assert_eq!(resources["test:file:///tmp/x"]["client"], "test");

    // getPrompt / readResource via with_client.
    let prompt = mcp
        .get_prompt("test", "greet", Some(json!({ "who": "world" })))
        .await
        .unwrap();
    assert!(prompt.is_some());
    let resource = mcp.read_resource("test", "file:///tmp/x").await.unwrap();
    assert!(resource.is_some());

    // OAuth state for a local server is not authenticated.
    assert_eq!(
        mcp.get_auth_status("test").await.unwrap(),
        AuthStatus::NotAuthenticated
    );

    mcp.close_all().await;
    let _ = std::fs::remove_dir_all(&dir);
}
