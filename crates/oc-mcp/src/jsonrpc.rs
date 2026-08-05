//! JSON-RPC 2.0 wire messages for the Model Context Protocol.
//!
//! Mirrors the `JSONRPCMessage` schema of `@modelcontextprotocol/sdk@1.29.0`
//! (`types.js`) which the reference uses in
//! `reference/packages/opencode/src/mcp/index.ts`.
//!
//! Messages are newline-delimited JSON on stdio and JSON/SSE payloads on HTTP
//! transports. The protocol also carries server notifications such as
//! `notifications/message` and `notifications/tools/list_changed`, and client
//! requests such as `ping` and `roots/list` that must be answered by the client.

use serde::{Deserialize, Serialize};

pub const JSONRPC_VERSION: &str = "2.0";

// Standard JSON-RPC 2.0 error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// A JSON-RPC request or notification id. The SDK accepts `string | number`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    Str(String),
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestId::Number(n) => write!(f, "{n}"),
            RequestId::Str(s) => write!(f, "{s}"),
        }
    }
}

/// `{ jsonrpc: "2.0", id, method, params? }` — an outbound request.
/// From reference: `JSONRPCRequestSchema` in `@modelcontextprotocol/sdk/types.js`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// `{ jsonrpc: "2.0", method, params? }` — a one-way notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// `{ jsonrpc: "2.0", id, result }` — a successful response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub id: RequestId,
    pub result: serde_json::Value,
}

/// `{ jsonrpc: "2.0", id, error }` — an error response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub id: RequestId,
    pub error: JsonRpcError,
}

/// The JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Any JSON-RPC message that can travel over an MCP transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    Response(Response),
    Error(ErrorResponse),
    Request(Request),
    Notification(Notification),
}

impl Message {
    pub fn request(
        id: impl Into<RequestId>,
        method: impl Into<String>,
        params: Option<serde_json::Value>,
    ) -> Self {
        Message::Request(Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            method: method.into(),
            params,
        })
    }

    pub fn response(id: RequestId, result: serde_json::Value) -> Self {
        Message::Response(Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result,
        })
    }

    pub fn error_response(id: RequestId, error: JsonRpcError) -> Self {
        Message::Error(ErrorResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            error,
        })
    }

    pub fn notification(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Message::Notification(Notification {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        })
    }

    /// Serialize to a single stdio line (the SDK writes `JSON.stringify(msg) + "\n"`).
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("JSON-RPC message is serializable")
    }

    /// The request id, if this is a request or a response.
    pub fn id(&self) -> Option<&RequestId> {
        match self {
            Message::Request(r) => Some(&r.id),
            Message::Response(r) => Some(&r.id),
            Message::Error(r) => Some(&r.id),
            Message::Notification(_) => None,
        }
    }
}

impl From<u64> for RequestId {
    fn from(value: u64) -> Self {
        RequestId::Number(value)
    }
}

impl From<String> for RequestId {
    fn from(value: String) -> Self {
        RequestId::Str(value)
    }
}

impl From<&str> for RequestId {
    fn from(value: &str) -> Self {
        RequestId::Str(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// From reference: the SDK builds `{ jsonrpc: "2.0", id, method, params }`
    /// for `initialize` and omits `params` when undefined (zod optional keys are
    /// stripped). Golden JSON in these tests is derived from
    /// `@modelcontextprotocol/sdk@1.29.0` request/response shapes used by
    /// `reference/packages/opencode/src/mcp/index.ts`.
    ///
    /// Note: the workspace enables serde_json `preserve_order`, so keys inside
    /// `params` (a `serde_json::Value`) are emitted in insertion order, matching
    /// the reference JS object literal order. Field order does not affect MCP
    /// wire semantics (JSON objects are unordered).
    #[test]
    fn request_with_params_serializes_exactly() {
        let msg = Message::request(
            1u64,
            "initialize",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": { "roots": {} },
                "clientInfo": { "name": "opencode", "version": "0.1.0" },
            })),
        );
        assert_eq!(
            msg.to_line(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{"roots":{}},"clientInfo":{"name":"opencode","version":"0.1.0"}}}"#
        );
    }

    #[test]
    fn request_without_params_omits_params() {
        let msg = Message::request(2u64, "tools/list", None);
        assert_eq!(
            msg.to_line(),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#
        );
    }

    #[test]
    fn notification_serializes_exactly() {
        let msg = Message::notification("notifications/initialized", None);
        assert_eq!(
            msg.to_line(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
        );
    }

    #[test]
    fn roundtrip_all_kinds() {
        let cases = [
            Message::request(1u64, "initialize", Some(json!({ "a": 1 }))),
            Message::request("str-id", "ping", None),
            Message::response(RequestId::Number(2), json!({ "ok": true })),
            Message::error_response(
                RequestId::Number(3),
                JsonRpcError {
                    code: METHOD_NOT_FOUND,
                    message: "Method not found".into(),
                    data: None,
                },
            ),
            Message::notification("notifications/initialized", None),
        ];
        for case in cases {
            let line = case.to_line();
            let parsed: Message = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed.to_line(), line);
        }
    }

    #[test]
    fn parses_server_response_with_string_id() {
        let line = r#"{"jsonrpc":"2.0","id":"abc","result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"test","version":"1.0.0"}}}"#;
        let msg: Message = serde_json::from_str(line).unwrap();
        assert!(matches!(msg, Message::Response(_)));
        assert_eq!(msg.id(), Some(&RequestId::Str("abc".into())));
    }
}
