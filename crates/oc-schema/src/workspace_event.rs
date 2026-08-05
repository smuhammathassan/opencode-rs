//! From reference/packages/schema/src/workspace-event.ts

use crate::define_event;
use crate::event::Definition;
use crate::workspace_id::WorkspaceID;
use serde::{Deserialize, Serialize};

/// `WorkspaceEvent.ConnectionStatus.status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "connecting")]
    Connecting,
    #[serde(rename = "disconnected")]
    Disconnected,
    #[serde(rename = "error")]
    Error,
}

/// `WorkspaceEvent.ConnectionStatus`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConnectionStatus {
    #[serde(rename = "workspaceID")]
    pub workspace_id: WorkspaceID,
    pub status: Status,
}

define_event! {
    /// `workspace.ready`.
    pub struct Ready {
        tag: ReadyTag,
        r#type: "workspace.ready",
        data: ReadyData,
    }
}

define_event! {
    /// `workspace.failed`.
    pub struct Failed {
        tag: FailedTag,
        r#type: "workspace.failed",
        data: FailedData,
    }
}

define_event! {
    /// `workspace.status`.
    pub struct StatusEvent {
        tag: StatusEventTag,
        r#type: "workspace.status",
        data: ConnectionStatus,
    }
}

/// Payload of `workspace.ready`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ReadyData {
    pub name: String,
}

/// Payload of `workspace.failed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct FailedData {
    pub message: String,
}

/// `WorkspaceEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    Definition {
        r#type: "workspace.ready",
        durable: None,
    },
    Definition {
        r#type: "workspace.failed",
        durable: None,
    },
    Definition {
        r#type: "workspace.status",
        durable: None,
    },
];
