//! Tiny stdio language server used by oc-project's JSON-RPC integration test.
//! It intentionally implements only the lifecycle and a few deterministic
//! test methods; it is not part of the production OpenCode runtime.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

type Writer = Arc<Mutex<tokio::io::Stdout>>;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let writer = Arc::new(Mutex::new(tokio::io::stdout()));
    let mut opened_documents = HashSet::new();

    while let Some(message) = read_message(&mut reader).await {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        let id = message.get("id").cloned();
        match (method, id) {
            ("initialize", Some(id)) => {
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "capabilities": {
                                "hoverProvider": true,
                                "definitionProvider": true,
                            },
                            "serverInfo": { "name": "oc-project-fake-lsp" },
                        }
                    }),
                )
                .await;
            }
            ("shutdown", Some(id)) => {
                respond(
                    writer.clone(),
                    json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }),
                )
                .await;
            }
            ("test/slow", Some(id)) => {
                tokio::spawn(delayed_response(
                    writer.clone(),
                    id,
                    "slow",
                    std::time::Duration::from_millis(50),
                ));
            }
            ("test/fast", Some(id)) => {
                tokio::spawn(delayed_response(
                    writer.clone(),
                    id,
                    "fast",
                    std::time::Duration::from_millis(5),
                ));
            }
            ("test/error", Some(id)) => {
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32800, "message": "fake request failed" }
                    }),
                )
                .await;
            }
            ("test/server-notification", Some(id)) => {
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "method": "window/logMessage",
                        "params": { "type": 3, "message": "server notification" },
                    }),
                )
                .await;
                respond(
                    writer.clone(),
                    json!({ "jsonrpc": "2.0", "id": id, "result": { "value": "ack" } }),
                )
                .await;
            }
            ("test/server-request", Some(id)) => {
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "id": 9001,
                        "method": "workspace/configuration",
                        "params": { "items": [] },
                    }),
                )
                .await;
                respond(
                    writer.clone(),
                    json!({ "jsonrpc": "2.0", "id": id, "result": { "value": "ack" } }),
                )
                .await;
            }
            ("textDocument/didOpen", None) => {
                if let Some(uri) = message
                    .pointer("/params/textDocument/uri")
                    .and_then(Value::as_str)
                {
                    opened_documents.insert(uri.to_string());
                }
            }
            ("textDocument/didChange", None) => {}
            ("textDocument/hover", Some(id)) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let uri = params
                    .pointer("/textDocument/uri")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "opened": opened_documents.contains(uri),
                            "params": params,
                        },
                    }),
                )
                .await;
            }
            ("textDocument/prepareCallHierarchy", Some(id)) => {
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": [{
                            "name": "main",
                            "kind": 12,
                            "uri": "file:///workspace/main.rs",
                            "range": {
                                "start": { "line": 0, "character": 0 },
                                "end": { "line": 0, "character": 10 }
                            },
                            "selectionRange": {
                                "start": { "line": 0, "character": 3 },
                                "end": { "line": 0, "character": 7 }
                            }
                        }]
                    }),
                )
                .await;
            }
            ("callHierarchy/incomingCalls", Some(id))
            | ("callHierarchy/outgoingCalls", Some(id)) => {
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": [{ "from": { "name": "caller" } }]
                    }),
                )
                .await;
            }
            ("exit", None) => break,
            (_, Some(id)) => {
                respond(
                    writer.clone(),
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": "method not found" }
                    }),
                )
                .await;
            }
            _ => {}
        }
    }
}

async fn delayed_response(writer: Writer, id: Value, value: &str, delay: std::time::Duration) {
    tokio::time::sleep(delay).await;
    respond(
        writer,
        json!({ "jsonrpc": "2.0", "id": id, "result": { "value": value } }),
    )
    .await;
}

async fn respond(writer: Writer, message: Value) {
    let payload = serde_json::to_vec(&message).unwrap();
    let mut frame = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
    frame.extend_from_slice(&payload);
    let mut writer = writer.lock().await;
    writer.write_all(&frame).await.unwrap();
    writer.flush().await.unwrap();
}

async fn read_message<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Option<Value> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().ok()?);
        }
    }
    let mut payload = vec![0u8; length?];
    reader.read_exact(&mut payload).await.ok()?;
    serde_json::from_slice(&payload).ok()
}
