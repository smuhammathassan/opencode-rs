//! From reference/packages/schema/src/filesystem-watcher.ts

use crate::define_event;
use serde::{Deserialize, Serialize};

/// `FileSystemWatcher.event`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum WatcherEvent {
    #[serde(rename = "add")]
    Add,
    #[serde(rename = "change")]
    Change,
    #[serde(rename = "unlink")]
    Unlink,
}

define_event! {
    /// `file.watcher.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "file.watcher.updated",
        data: UpdatedData,
    }
}

/// Payload of `file.watcher.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct UpdatedData {
    pub file: String,
    pub event: WatcherEvent,
}

/// `FileSystemWatcher.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Updated;
    pub use crate::event::Definition;

    /// `FileSystemWatcher.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "file.watcher.updated",
        durable: None,
    }];
}

/// `FileSystemWatcher.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
