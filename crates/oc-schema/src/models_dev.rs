//! From reference/packages/schema/src/models-dev.ts

use crate::define_event;

define_event! {
    /// `models-dev.refreshed`.
    pub struct Refreshed {
        tag: RefreshedTag,
        r#type: "models-dev.refreshed",
        data: crate::schema::Empty,
    }
}

/// `ModelsDev.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Refreshed;
    pub use crate::event::Definition;

    /// `ModelsDev.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "models-dev.refreshed",
        durable: None,
    }];
}

/// `ModelsDev.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
