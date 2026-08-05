//! From reference/packages/schema/src/server-event.ts

use crate::define_event;
use crate::event::Definition;

define_event! {
    /// `server.connected`.
    pub struct Connected {
        tag: ConnectedTag,
        r#type: "server.connected",
        data: crate::schema::Empty,
    }
}

define_event! {
    /// `global.disposed`.
    pub struct Disposed {
        tag: DisposedTag,
        r#type: "global.disposed",
        data: crate::schema::Empty,
    }
}

/// `ServerEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    Definition {
        r#type: "server.connected",
        durable: None,
    },
    Definition {
        r#type: "global.disposed",
        durable: None,
    },
];
