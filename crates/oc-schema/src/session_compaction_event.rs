//! From reference/packages/schema/src/session-compaction-event.ts

use crate::define_event;
use crate::event::Definition;
use crate::session_id::SessionID;

define_event! {
    /// `session.compacted`.
    pub struct Compacted {
        tag: CompactedTag,
        r#type: "session.compacted",
        data: CompactedData,
    }
}

/// Payload of `session.compacted`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct CompactedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
}

/// `SessionCompactionEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[Definition {
    r#type: "session.compacted",
    durable: None,
}];
