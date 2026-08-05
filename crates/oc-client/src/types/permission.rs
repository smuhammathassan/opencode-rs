//! Permission types.
//! From reference/packages/schema/src/permission.ts.

use crate::types::schema::JsonValue;
use std::collections::HashMap;

/// `Permission.Effect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionEffect {
    Allow,
    Deny,
    Ask,
}

/// `Permission.Reply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReply {
    Once,
    Always,
    Reject,
}

/// `Permission.Source` — currently only `{ type: "tool" }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSource {
    #[serde(rename = "type")]
    pub kind: PermissionSourceType,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionSourceType {
    Tool,
}

/// `Permission.Request`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub action: String,
    pub resources: Vec<String>,
    #[serde(default)]
    pub save: Option<Vec<String>>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, JsonValue>>,
    #[serde(default)]
    pub source: Option<PermissionSource>,
}

/// The payload accepted by `session.permission.create` (request without the
/// server-assigned `sessionID`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCreatePayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub action: String,
    pub resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, JsonValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PermissionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// `PermissionsCreateInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionsCreateInput {
    pub session_id: String,
    pub id: Option<String>,
    pub action: String,
    pub resources: Vec<String>,
    pub save: Option<Vec<String>>,
    pub metadata: Option<HashMap<String, JsonValue>>,
    pub source: Option<PermissionSource>,
    pub agent: Option<String>,
}

/// `PermissionsListInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionsListInput {
    pub session_id: String,
}

/// `PermissionsGetInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionsGetInput {
    pub session_id: String,
    pub request_id: String,
}

/// The response of `session.permission.create`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDecision {
    pub id: String,
    pub effect: PermissionEffect,
}

/// `PermissionsReplyInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionsReplyInput {
    pub session_id: String,
    pub request_id: String,
    pub reply: PermissionReply,
    pub message: Option<String>,
}

/// `Permission.Rule`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub action: String,
    pub resource: String,
    pub effect: PermissionEffect,
}
