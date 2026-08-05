//! From reference/packages/schema/src/worktree-event.ts

use crate::define_event;
use crate::event::Definition;

define_event! {
    /// `worktree.ready`.
    pub struct Ready {
        tag: ReadyTag,
        r#type: "worktree.ready",
        data: ReadyData,
    }
}

define_event! {
    /// `worktree.failed`.
    pub struct Failed {
        tag: FailedTag,
        r#type: "worktree.failed",
        data: FailedData,
    }
}

/// Payload of `worktree.ready`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ReadyData {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub branch: Option<String>,
}

/// Payload of `worktree.failed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct FailedData {
    pub message: String,
}

/// `WorktreeEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    Definition {
        r#type: "worktree.ready",
        durable: None,
    },
    Definition {
        r#type: "worktree.failed",
        durable: None,
    },
];
