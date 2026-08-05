//! From reference/packages/schema/src/project-directories.ts

use crate::define_event;
use crate::project;

define_event! {
    /// `project.directories.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "project.directories.updated",
        data: UpdatedData,
    }
}

/// Payload of `project.directories.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct UpdatedData {
    #[serde(rename = "projectID")]
    pub project_id: project::ID,
}

/// `ProjectDirectories.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use crate::event::Definition;
    pub use super::Updated;

    /// `ProjectDirectories.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "project.directories.updated",
        durable: None,
    }];
}

/// `ProjectDirectories.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
