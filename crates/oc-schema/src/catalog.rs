//! From reference/packages/schema/src/catalog.ts

use crate::define_event;

define_event! {
    /// `catalog.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "catalog.updated",
        data: crate::schema::Empty,
    }
}

/// `Catalog.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Updated;
    pub use crate::event::Definition;

    /// `Catalog.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "catalog.updated",
        durable: None,
    }];
}

/// `Catalog.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
