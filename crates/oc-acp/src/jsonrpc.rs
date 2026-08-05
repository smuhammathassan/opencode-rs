//! JSON-RPC 2.0 envelope types.
//!
//! From `@agentclientprotocol/sdk` `dist/jsonrpc.d.ts`. opencode speaks ACP over
//! newline-delimited JSON-RPC 2.0 messages.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request or notification identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
    Null,
}

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 success response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    pub result: Value,
}

/// A JSON-RPC 2.0 error response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcErrorResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    pub error: crate::types::RequestError,
}

/// A JSON-RPC 2.0 message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcMessage {
    Request(RpcRequest),
    Notification(RpcNotification),
    Response(RpcResponse),
    Error(RpcErrorResponse),
}

impl RpcMessage {
    /// Whether this message is a response to a request carrying `id`.
    pub fn id(&self) -> Option<&RequestId> {
        match self {
            RpcMessage::Request(message) => Some(&message.id),
            RpcMessage::Response(message) => Some(&message.id),
            RpcMessage::Error(message) => Some(&message.id),
            RpcMessage::Notification(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_request() {
        let message = RpcMessage::Request(RpcRequest {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(1),
            method: "initialize".into(),
            params: Some(json!({ "protocolVersion": 1 })),
        });
        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": 1 }
            })
        );
    }

    #[test]
    fn encodes_notification() {
        let message = RpcMessage::Notification(RpcNotification {
            jsonrpc: "2.0".into(),
            method: "session/cancel".into(),
            params: Some(json!({ "sessionId": "abc" })),
        });
        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": "abc" }
            })
        );
    }

    #[test]
    fn encodes_response() {
        let message = RpcMessage::Response(RpcResponse {
            jsonrpc: "2.0".into(),
            id: RequestId::String("id-1".into()),
            result: json!({ "protocolVersion": 1 }),
        });
        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            json!({
                "jsonrpc": "2.0",
                "id": "id-1",
                "result": { "protocolVersion": 1 }
            })
        );
    }

    #[test]
    fn decodes_request_and_notification() {
        let message: RpcMessage = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}"#,
        )
        .unwrap();
        assert!(matches!(message, RpcMessage::Request(_)));
        let message: RpcMessage = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"abc"}}"#,
        )
        .unwrap();
        assert!(matches!(message, RpcMessage::Notification(_)));
    }
}
