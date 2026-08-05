//! From reference/packages/schema/src/lsp-event.ts

use crate::define_event;
use crate::event::Definition;

define_event! {
    /// `lsp.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "lsp.updated",
        data: crate::schema::Empty,
    }
}

/// `LspEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[Definition {
    r#type: "lsp.updated",
    durable: None,
}];
