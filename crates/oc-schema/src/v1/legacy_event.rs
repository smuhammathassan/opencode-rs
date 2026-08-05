//! From reference/packages/schema/src/v1/legacy-event.ts

use crate::define_event;
use crate::event::Definition;
use crate::session_id::SessionID;
use crate::v1::session;

define_event! {
    /// `command.executed`.
    pub struct CommandExecuted {
        tag: CommandExecutedTag,
        r#type: "command.executed",
        data: CommandExecutedData,
    }
}

/// Payload of `command.executed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct CommandExecutedData {
    pub name: String,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub arguments: String,
    #[serde(rename = "messageID")]
    pub message_id: session::MessageID,
}

/// `LegacyEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[Definition {
    r#type: "command.executed",
    durable: None,
}];
