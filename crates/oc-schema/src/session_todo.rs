//! From reference/packages/schema/src/session-todo.ts

use crate::define_event;
use crate::session_id::SessionID;

/// `SessionTodo.Info`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub content: String,
    pub status: String,
    pub priority: String,
}

define_event! {
    /// `todo.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "todo.updated",
        data: UpdatedData,
    }
}

/// Payload of `todo.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct UpdatedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub todos: Vec<Info>,
}

/// `SessionTodo.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Updated;
    pub use crate::event::Definition;

    /// `SessionTodo.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "todo.updated",
        durable: None,
    }];
}

/// `SessionTodo.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
