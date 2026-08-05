//! From reference/packages/schema/src/ide-event.ts

use crate::define_event;
use crate::event::Definition;

define_event! {
    /// `ide.installed`.
    pub struct Installed {
        tag: InstalledTag,
        r#type: "ide.installed",
        data: InstalledData,
    }
}

/// Payload of `ide.installed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct InstalledData {
    pub ide: String,
}

/// `IdeEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[Definition {
    r#type: "ide.installed",
    durable: None,
}];
