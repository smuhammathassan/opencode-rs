//! From reference/packages/schema/src/workspace.ts

/// `Workspace.ID`.
pub type ID = crate::workspace_id::WorkspaceID;

/// `Workspace.Event` — the workspace event namespace.
pub use crate::workspace_event::DEFINITIONS;
pub use crate::workspace_event::{
    ConnectionStatus, Failed, FailedData, Ready, ReadyData, Status, StatusEvent,
};
