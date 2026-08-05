//! From reference/packages/schema/src/plugin.ts

use crate::define_event;

/// `Plugin.ID`.
pub type ID = String;

define_event! {
    /// `plugin.added`.
    pub struct Added {
        tag: AddedTag,
        r#type: "plugin.added",
        data: AddedData,
    }
}

/// Payload of `plugin.added`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct AddedData {
    pub id: ID,
}

/// `Plugin.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Added;
    pub use crate::event::Definition;

    /// `Plugin.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "plugin.added",
        durable: None,
    }];
}

/// `Plugin.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
