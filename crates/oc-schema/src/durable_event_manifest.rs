//! From reference/packages/schema/src/durable-event-manifest.ts

use crate::event::{durable, Definition};
use indexmap::IndexMap;

/// `DurableEventManifest.SessionDurable.schema` — the durable session event union type.
pub type SessionDurableEvent = crate::session_event::DurableEvent;

/// `SessionDurable.definitions` — the durable session event definitions keyed by `type.version`.
pub fn session_durable_definitions() -> IndexMap<String, Definition> {
    durable(crate::session_event::DURABLE_DEFINITIONS)
}

/// `DurableEventManifest.Durable` — every durable event (V1 + next), keyed by `type.version`.
pub fn durable_definitions() -> IndexMap<String, Definition> {
    let mut v1_durable: Vec<Definition> = crate::session_v1::Definitions
        .iter()
        .filter(|d| d.durable.is_some())
        .cloned()
        .collect();
    v1_durable.extend_from_slice(crate::session_event::DURABLE_DEFINITIONS);
    durable(&v1_durable)
}
