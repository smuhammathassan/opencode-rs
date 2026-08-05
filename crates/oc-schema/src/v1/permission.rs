//! From reference/packages/schema/src/v1/permission.ts

use crate::define_event;
use crate::project_id::ProjectID;
use crate::session_id::SessionID;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// `PermissionV1.ID` — starts with `per`.
pub type ID = String;

/// `Permission.ID.ascending(id?)`.
pub fn ascending(id: Option<String>) -> ID {
    match id {
        Some(id) => id,
        None => format!("per_{}", crate::identifier::ascending()),
    }
}

/// `PermissionAction`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

/// `PermissionRule`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rule {
    pub permission: String,
    pub pattern: String,
    pub action: Action,
}

/// `PermissionRuleset`.
pub type Ruleset = Vec<Rule>;

/// `PermissionRequest.tool`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tool {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

/// `PermissionRequest`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub id: ID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub permission: String,
    pub patterns: Vec<String>,
    pub metadata: IndexMap<String, JsonValue>,
    pub always: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool: Option<Tool>,
}

/// `PermissionV1.Reply`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    #[serde(rename = "once")]
    Once,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "reject")]
    Reject,
}

/// `PermissionReplyBody`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReplyBody {
    pub reply: Reply,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message: Option<String>,
}

/// `PermissionApproval`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Approval {
    #[serde(rename = "projectID")]
    pub project_id: ProjectID,
    pub patterns: Vec<String>,
}

/// `PermissionAskInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AskInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<ID>,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub permission: String,
    pub patterns: Vec<String>,
    pub metadata: IndexMap<String, JsonValue>,
    pub always: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool: Option<Tool>,
    pub ruleset: Ruleset,
}

/// `PermissionReplyInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReplyInput {
    #[serde(rename = "requestID")]
    pub request_id: ID,
    pub reply: Reply,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message: Option<String>,
}

define_event! {
    /// `permission.asked`.
    pub struct Asked {
        tag: AskedTag,
        r#type: "permission.asked",
        data: Request,
    }
}

define_event! {
    /// `permission.replied`.
    pub struct Replied {
        tag: RepliedTag,
        r#type: "permission.replied",
        data: RepliedData,
    }
}

/// Payload of `permission.replied`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct RepliedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "requestID")]
    pub request_id: ID,
    pub reply: Reply,
}

/// `PermissionV1.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use crate::event::Definition;
    pub use super::{Asked, Replied};

    /// `PermissionV1.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[
        Definition {
            r#type: "permission.asked",
            durable: None,
        },
        Definition {
            r#type: "permission.replied",
            durable: None,
        },
    ];
}

/// `PermissionV1.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
