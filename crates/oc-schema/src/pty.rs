//! From reference/packages/schema/src/pty.ts

use crate::define_event;
use crate::identifier::ascending;
use crate::schema::{NonNegativeInt, PositiveInt};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// `Pty.ID` — starts with `pty`.
pub type ID = String;

/// `Pty.ID.create()`.
pub fn create_id() -> ID {
    format!("pty_{}", ascending())
}

/// `Pty.Info.status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "exited")]
    Exited,
}

/// `Pty.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: Status,
    pub pid: NonNegativeInt,
    #[serde(rename = "exitCode", skip_serializing_if = "Option::is_none", default)]
    pub exit_code: Option<NonNegativeInt>,
}

define_event! {
    /// `pty.created`.
    pub struct Created {
        tag: CreatedTag,
        r#type: "pty.created",
        data: InfoEventData,
    }
}

define_event! {
    /// `pty.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "pty.updated",
        data: InfoEventData,
    }
}

define_event! {
    /// `pty.exited`.
    pub struct Exited {
        tag: ExitedTag,
        r#type: "pty.exited",
        data: ExitedData,
    }
}

define_event! {
    /// `pty.deleted`.
    pub struct Deleted {
        tag: DeletedTag,
        r#type: "pty.deleted",
        data: DeletedData,
    }
}

/// Payload of `pty.created` / `pty.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct InfoEventData {
    pub info: Info,
}

/// Payload of `pty.exited`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ExitedData {
    pub id: ID,
    #[serde(rename = "exitCode")]
    pub exit_code: NonNegativeInt,
}

/// Payload of `pty.deleted`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct DeletedData {
    pub id: ID,
}

/// `Pty.CreateInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CreateInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub env: Option<IndexMap<String, String>>,
}

/// `Pty.UpdateInput.size`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Size {
    pub rows: PositiveInt,
    pub cols: PositiveInt,
}

/// `Pty.UpdateInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UpdateInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub size: Option<Size>,
}

/// `Pty.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use crate::event::Definition;
    pub use super::{Created, Deleted, Exited, Updated};

    /// `Pty.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[
        Definition {
            r#type: "pty.created",
            durable: None,
        },
        Definition {
            r#type: "pty.updated",
            durable: None,
        },
        Definition {
            r#type: "pty.exited",
            durable: None,
        },
        Definition {
            r#type: "pty.deleted",
            durable: None,
        },
    ];
}

/// `Pty.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
