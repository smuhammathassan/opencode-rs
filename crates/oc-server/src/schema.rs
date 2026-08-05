//! Local mirrors of the reference wire types.
//!
//! TODO(integration): promote to oc-schema once that crate grows these types.
//! Serialized shapes mirror the zod schemas under reference/packages/schema/src/.
//! Optional fields are omitted when `None` exactly like the reference `optional(...)` helper.

use serde::{Deserialize, Serialize};

/// Model reference. From reference/packages/schema/src/model.ts (`Model.Ref`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Location reference. From reference/packages/schema/src/location.ts (`Location.Ref`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocationRef {
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

/// Location info. From reference/packages/schema/src/location.ts (`Location.Info`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub project: ProjectRef,
}

/// Project reference inside `Location.Info`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub id: String,
    pub directory: String,
}

/// The `{ location, data }` wrapper returned by location-scoped v2 endpoints.
/// From reference/packages/schema/src/location.ts (`Location.response`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocationResponse<T> {
    pub location: LocationInfo,
    pub data: T,
}

/// Token counts. From reference/packages/schema/src/session.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: CacheTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CacheTokens {
    pub read: f64,
    pub write: f64,
}

/// Timestamps are epoch milliseconds. From reference/packages/schema/src/schema.ts
/// (`DateTimeUtcFromMillis`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTime {
    pub created: i64,
    pub updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<i64>,
}

/// Session info. From reference/packages/schema/src/session.ts (`Session.Info`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    pub cost: f64,
    pub tokens: Tokens,
    pub time: SessionTime,
    pub title: String,
    pub location: LocationRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert: Option<serde_json::Value>,
}

/// Paginated session list response. From reference/packages/protocol/src/groups/session.ts
/// (`SessionsResponse`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionsResponse {
    pub data: Vec<SessionInfo>,
    pub cursor: SessionCursor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionCursor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// `{ data: Session.Info }` body for single-session endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionData {
    pub data: SessionInfo,
}

/// Active sessions map. From reference/packages/protocol/src/groups/session.ts
/// (`SessionActive`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionsActive {
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// Message list response with opaque cursors. From reference/packages/protocol/src/groups/message.ts
/// (`SessionMessagesResponse`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMessagesResponse {
    pub data: Vec<serde_json::Value>,
    pub cursor: SessionCursor,
}

/// Message list response where `data` carries one message. From
/// reference/packages/protocol/src/groups/session.ts (`session.message`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageData {
    pub data: serde_json::Value,
}

/// Context messages list. From reference/packages/protocol/src/groups/session.ts
/// (`session.context`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextData {
    pub data: Vec<serde_json::Value>,
}

/// Admitted session input. From reference/packages/schema/src/session-input.ts
/// (`SessionInput.Admitted`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Admitted {
    pub admitted_seq: i64,
    pub id: String,
    pub session_id: String,
    pub prompt: serde_json::Value,
    pub delivery: String,
    pub time_created: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promoted_seq: Option<i64>,
}

/// Health output. From reference/packages/protocol/src/groups/health.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthOutput {
    pub healthy: bool,
}

/// Session history page. From reference/packages/protocol/src/groups/session.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionHistory {
    pub data: Vec<serde_json::Value>,
    pub has_more: bool,
}

/// A permission evaluation result. From reference/packages/protocol/src/groups/permission.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionCreateData {
    pub data: PermissionEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionEffect {
    pub id: String,
    pub effect: String,
}

/// Saved-permission list response. From reference/packages/protocol/src/groups/permission.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionSavedData {
    pub data: Vec<serde_json::Value>,
}

/// PTY connect token. From reference/packages/schema/src/pty-ticket.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectToken {
    pub token: String,
    pub pty_id: String,
    pub expires_at: i64,
}

/// Minimal helpers for message payloads (SessionMessage tagged union).
pub mod message {
    use serde_json::{json, Value};

    /// `Session.Message.User` from reference/packages/schema/src/session-message.ts.
    pub fn user(id: &str, created: i64, text: &str) -> Value {
        json!({
            "id": id,
            "time": { "created": created },
            "type": "user",
            "text": text,
            "files": [],
            "agents": [],
        })
    }

    /// `Session.Message.System`.
    pub fn system(id: &str, created: i64, text: &str) -> Value {
        json!({
            "id": id,
            "time": { "created": created },
            "type": "system",
            "text": text,
        })
    }

    /// `Session.Message.AgentSwitched`.
    pub fn agent_switched(id: &str, created: i64, agent: &str) -> Value {
        json!({
            "id": id,
            "time": { "created": created },
            "type": "agent-switched",
            "agent": agent,
        })
    }

    /// `Session.Message.ModelSwitched`.
    pub fn model_switched(id: &str, created: i64, model: &crate::schema::ModelRef) -> Value {
        json!({
            "id": id,
            "time": { "created": created },
            "type": "model-switched",
            "model": model,
        })
    }
}
