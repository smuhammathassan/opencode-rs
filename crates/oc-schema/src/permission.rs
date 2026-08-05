//! From reference/packages/schema/src/permission.ts

use crate::define_event;
use crate::identifier::ascending;
use crate::session_id::SessionID;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// `PermissionV2.ID` — starts with `per`.
pub type ID = String;

/// `Permission.ID.create(id?)`.
pub fn create_id(id: Option<String>) -> ID {
    match id {
        Some(id) => id,
        None => format!("per_{}", ascending()),
    }
}

/// `PermissionV2.Source`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Source {
    #[serde(rename = "type")]
    pub r#type: SourceType,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SourceType {
    #[serde(rename = "tool")]
    Value,
}

/// `PermissionV2.Request`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub id: ID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub action: String,
    pub resources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub save: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<Source>,
}

impl Request {
    /// `Permission.Request.fields` — all properties except `id`.
    pub fn fields(&self) -> RequestFields {
        RequestFields {
            session_id: self.session_id.clone(),
            action: self.action.clone(),
            resources: self.resources.clone(),
            save: self.save.clone(),
            metadata: self.metadata.clone(),
            source: self.source.clone(),
        }
    }
}

/// The `RequestFields` subset used by the `permission.v2.asked` event payload.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RequestFields {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub action: String,
    pub resources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub save: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<Source>,
}

/// `PermissionV2.Reply`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    #[serde(rename = "once")]
    Once,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "reject")]
    Reject,
}

/// `PermissionV2.Effect`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

/// `PermissionV2.Rule`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rule {
    pub action: String,
    pub resource: String,
    pub effect: Effect,
}

/// `PermissionV2.Ruleset`.
pub type Ruleset = Vec<Rule>;

define_event! {
    /// `permission.v2.asked`.
    pub struct Asked {
        tag: AskedTag,
        r#type: "permission.v2.asked",
        data: RequestFields,
    }
}

define_event! {
    /// `permission.v2.replied`.
    pub struct Replied {
        tag: RepliedTag,
        r#type: "permission.v2.replied",
        data: RepliedData,
    }
}

/// Payload of `permission.v2.replied`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RepliedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "requestID")]
    pub request_id: ID,
    pub reply: Reply,
}

/// `Permission.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::{Asked, Replied};
    pub use crate::event::Definition;

    /// `Permission.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[
        Definition {
            r#type: "permission.v2.asked",
            durable: None,
        },
        Definition {
            r#type: "permission.v2.replied",
            durable: None,
        },
    ];
}

/// `Permission.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
