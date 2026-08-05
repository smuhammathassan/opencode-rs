//! From reference/packages/schema/src/installation-event.ts

use crate::define_event;
use crate::event::Definition;

define_event! {
    /// `installation.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "installation.updated",
        data: VersionData,
    }
}

define_event! {
    /// `installation.update-available`.
    pub struct UpdateAvailable {
        tag: UpdateAvailableTag,
        r#type: "installation.update-available",
        data: VersionData,
    }
}

/// Payload of installation events.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct VersionData {
    pub version: String,
}

/// `InstallationEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    Definition {
        r#type: "installation.updated",
        durable: None,
    },
    Definition {
        r#type: "installation.update-available",
        durable: None,
    },
];
