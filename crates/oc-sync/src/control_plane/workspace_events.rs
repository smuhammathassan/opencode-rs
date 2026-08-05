//! Workspace events.
//!
//! From reference/packages/schema/src/workspace-event.ts.
//!
//! TODO(integration): promote to oc-schema.

use serde::{Deserialize, Serialize};

/// `WorkspaceEvent.ConnectionStatus` from reference/packages/schema/src/workspace-event.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "connecting")]
    Connecting,
    #[serde(rename = "disconnected")]
    Disconnected,
    #[serde(rename = "error")]
    Error,
}

/// The `workspace.status` event payload (`ConnectionStatus.fields`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusPayload {
    #[serde(rename = "workspaceID")]
    pub workspace_id: String,
    pub status: ConnectionStatus,
}

/// The `workspace.ready` event payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadyPayload {
    pub name: String,
}

/// The `workspace.failed` event payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FailedPayload {
    pub message: String,
}

pub const STATUS_TYPE: &str = "workspace.status";
pub const READY_TYPE: &str = "workspace.ready";
pub const FAILED_TYPE: &str = "workspace.failed";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_status_json() {
        assert_eq!(
            serde_json::to_string(&ConnectionStatus::Connected).unwrap(),
            "\"connected\""
        );
        assert_eq!(
            serde_json::to_string(&ConnectionStatus::Error).unwrap(),
            "\"error\""
        );
    }

    #[test]
    fn status_payload_json() {
        let status = StatusPayload {
            workspace_id: "wrk_1".into(),
            status: ConnectionStatus::Connecting,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#"{"workspaceID":"wrk_1","status":"connecting"}"#);
    }
}
