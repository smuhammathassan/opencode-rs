//! Session types.
//! From reference/packages/schema/src/session.ts.

// TODO(integration): promote to oc-schema.
use crate::types::model::ModelRef;
use crate::types::revert::RevertState;
use crate::types::schema::{DateTimeMillis, Order, RelativePath};

/// `Session.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    #[serde(rename = "parentID")]
    pub parent_id: Option<String>,
    #[serde(rename = "projectID")]
    pub project_id: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<ModelRef>,
    pub cost: f64,
    pub tokens: SessionTokens,
    pub time: SessionTime,
    pub title: String,
    pub location: SessionLocation,
    #[serde(default)]
    pub subpath: Option<RelativePath>,
    #[serde(default)]
    pub revert: Option<RevertState>,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: SessionTokenCache,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenCache {
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTime {
    pub created: DateTimeMillis,
    pub updated: DateTimeMillis,
    #[serde(default)]
    pub archived: Option<DateTimeMillis>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLocation {
    pub directory: String,
    #[serde(default)]
    #[serde(rename = "workspaceID")]
    pub workspace_id: Option<String>,
}

/// The `cursor` field of session/message listing responses.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseCursor {
    #[serde(default)]
    pub previous: Option<String>,
    #[serde(default)]
    pub next: Option<String>,
}

/// `SessionsResponse` — `{ data, cursor }`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsResponse {
    pub data: Vec<SessionInfo>,
    pub cursor: ResponseCursor,
}

/// `{ type: "running" }` — session activity marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActive {
    #[serde(rename = "type")]
    pub kind: SessionActiveType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionActiveType {
    Running,
}

/// `SessionsListInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionsListInput {
    pub workspace: Option<String>,
    pub limit: Option<u64>,
    pub order: Option<Order>,
    pub search: Option<String>,
    pub directory: Option<String>,
    pub project: Option<String>,
    pub subpath: Option<String>,
    pub cursor: Option<String>,
}

/// The `{ sessionID }` path-only input shared by several session endpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionIDInput {
    pub session_id: String,
}

/// `SessionsGetInput`.
pub type SessionsGetInput = SessionIDInput;
/// `SessionsCompactInput`.
pub type SessionsCompactInput = SessionIDInput;
/// `SessionsWaitInput`.
pub type SessionsWaitInput = SessionIDInput;
/// `SessionsClearInput`.
pub type SessionsClearInput = SessionIDInput;
/// `SessionsCommitInput`.
pub type SessionsCommitInput = SessionIDInput;
/// `SessionsContextInput`.
pub type SessionsContextInput = SessionIDInput;
/// `SessionsInterruptInput`.
pub type SessionsInterruptInput = SessionIDInput;

/// `SessionsSwitchAgentInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionsSwitchAgentInput {
    pub session_id: String,
    pub agent: String,
}

/// `SessionsSwitchModelInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionsSwitchModelInput {
    pub session_id: String,
    pub model: ModelRef,
}

/// `SessionsPromptInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionsPromptInput {
    pub session_id: String,
    pub id: Option<String>,
    pub prompt: crate::types::prompt::PromptInput,
    pub delivery: Option<crate::types::schema::Delivery>,
    pub resume: Option<bool>,
}

/// `SessionsStageInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionsStageInput {
    pub session_id: String,
    pub message_id: String,
    pub files: Option<bool>,
}

/// `SessionsEventsInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionsEventsInput {
    pub session_id: String,
    pub after: Option<u64>,
}

/// `SessionsMessageInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionsMessageInput {
    pub session_id: String,
    pub message_id: String,
}

/// `SessionsHistoryInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionsHistoryInput {
    pub session_id: String,
    pub limit: Option<u64>,
    pub after: Option<u64>,
}

/// `SessionsHistory` — `{ data, hasMore }`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsHistory {
    pub data: Vec<crate::types::event::SessionDurableEvent>,
    pub has_more: bool,
}

/// `SessionsCreateInput`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsCreateInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<SessionCreateLocation>,
}

/// Location payload for `session.create`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateLocation {
    pub directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "workspaceID")]
    pub workspace_id: Option<String>,
}
