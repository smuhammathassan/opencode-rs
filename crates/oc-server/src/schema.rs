//! Local mirrors of the reference wire types.
//!
//! TODO(integration): promote to oc-schema once that crate grows these types.
//! Serialized shapes mirror the zod schemas under reference/packages/schema/src/.
//! Optional fields are omitted when `None` exactly like the reference `optional(...)` helper.
//!
//! Note the reference mixes camelCase (`admittedSeq`, `timeCreated`) with acronym
//! suffixes (`projectID`, `providerID`, `sessionID`); field renames below replicate that.

use serde::{Deserialize, Serialize};

// Canonical home: `oc_schema`.
pub use oc_schema::location::Response as LocationResponse;
pub use oc_schema::location::{Info as LocationInfo, Project as ProjectRef, Ref as LocationRef};
pub use oc_schema::model::Ref as ModelRef;

/// Token counts. From reference/packages/schema/src/session.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: CacheTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheTokens {
    pub read: f64,
    pub write: f64,
}

/// Timestamps are epoch milliseconds. From reference/packages/schema/src/schema.ts
/// (`DateTimeUtcFromMillis`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTime {
    pub created: i64,
    pub updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<i64>,
}

/// Session info. From reference/packages/schema/src/session.ts (`Session.Info`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInfo {
    pub id: String,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(rename = "projectID")]
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

/// Context messages list. From reference/packages/protocol/src/groups/session.ts
/// (`session.context`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextData {
    pub data: Vec<serde_json::Value>,
}

/// Admitted session input. From reference/packages/schema/src/session-input.ts
/// (`SessionInput.Admitted`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Admitted {
    #[serde(rename = "admittedSeq")]
    pub admitted_seq: i64,
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub prompt: serde_json::Value,
    pub delivery: String,
    #[serde(rename = "timeCreated")]
    pub time_created: i64,
    #[serde(rename = "promotedSeq", skip_serializing_if = "Option::is_none")]
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
pub struct PermissionEffect {
    pub id: String,
    pub effect: String,
}

/// Saved-permission list response. From reference/packages/protocol/src/groups/permission.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionSavedData {
    pub data: Vec<oc_schema::permission_saved::Info>,
}

/// PTY connect token. From reference/packages/schema/src/pty-ticket.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectToken {
    pub token: String,
    #[serde(rename = "ptyID")]
    pub pty_id: String,
    #[serde(rename = "expiresAt")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_info_serializes_reference_names() {
        let info = SessionInfo {
            id: "ses_1".into(),
            parent_id: None,
            project_id: "prj_1".into(),
            agent: Some("build".into()),
            model: Some(ModelRef {
                id: "claude".into(),
                provider_id: "anthropic".into(),
                variant: None,
            }),
            cost: 1.5,
            tokens: Tokens {
                input: 10.0,
                output: 20.0,
                reasoning: 30.0,
                cache: CacheTokens {
                    read: 40.0,
                    write: 50.0,
                },
            },
            time: SessionTime {
                created: 1,
                updated: 2,
                archived: None,
            },
            title: "t".into(),
            location: LocationRef {
                directory: "/tmp".into(),
                workspace_id: Some("ws_1".into()),
            },
            subpath: None,
            revert: None,
        };
        let value = serde_json::to_value(&info).unwrap();
        assert!(
            value.get("projectID").is_some(),
            "projectID missing: {value}"
        );
        assert!(
            value["model"].get("providerID").is_some(),
            "model.providerID missing: {value}"
        );
        assert!(
            value["location"].get("workspaceID").is_some(),
            "location.workspaceID missing: {value}"
        );
        assert!(value.get("parentID").is_none());
        let expected = serde_json::json!({
            "id": "ses_1",
            "projectID": "prj_1",
            "agent": "build",
            "model": { "id": "claude", "providerID": "anthropic" },
            "cost": 1.5,
            "tokens": { "input": 10.0, "output": 20.0, "reasoning": 30.0, "cache": { "read": 40.0, "write": 50.0 } },
            "time": { "created": 1, "updated": 2 },
            "title": "t",
            "location": { "directory": "/tmp", "workspaceID": "ws_1" },
        });
        assert_eq!(value, expected);
    }

    #[test]
    fn admitted_serializes_reference_names() {
        let admitted = Admitted {
            admitted_seq: 1,
            id: "msg_1".into(),
            session_id: "ses_1".into(),
            prompt: serde_json::json!({ "text": "hi" }),
            delivery: "steer".into(),
            time_created: 3,
            promoted_seq: None,
        };
        let value = serde_json::to_value(&admitted).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "admittedSeq": 1,
                "id": "msg_1",
                "sessionID": "ses_1",
                "prompt": { "text": "hi" },
                "delivery": "steer",
                "timeCreated": 3,
            })
        );
    }

    #[test]
    fn location_response_shape() {
        let response = LocationResponse {
            location: LocationInfo {
                directory: "/tmp".into(),
                workspace_id: None,
                project: ProjectRef {
                    id: "prj_1".into(),
                    directory: "/tmp".into(),
                },
            },
            data: vec![1, 2, 3],
        };
        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "location": { "directory": "/tmp", "project": { "id": "prj_1", "directory": "/tmp" } },
                "data": [1, 2, 3],
            })
        );
    }
}
