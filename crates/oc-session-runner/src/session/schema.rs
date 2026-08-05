use serde::{Deserialize, Serialize};

/// Branded Session ID (`ses_...` in reference, kept untyped here).
/// /// From reference/packages/schema/src/session-id.ts
pub type SessionID = String;

/// `Model.Ref`
/// /// From reference/packages/schema/src/model.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// The minimal `Session.Info` projection the runner reads. Kept deliberately
/// narrow; the full contract lives in `oc-session`.
/// /// From reference/packages/schema/src/session.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: SessionID,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    pub location: LocationRef,
}

/// `Location.Ref`
/// /// From reference/packages/schema/src/location.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationRef {
    pub directory: String,
    #[serde(rename = "workspaceID", skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

/// The runtime Location identity the runner compares against.
/// /// From reference/packages/core/src/location.ts
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub directory: String,
    pub workspace_id: Option<String>,
}

impl Location {
    pub fn new(directory: impl Into<String>, workspace_id: Option<String>) -> Self {
        Self {
            directory: directory.into(),
            workspace_id,
        }
    }

    /// True when this location owns the given session.
    /// /// From reference/packages/core/src/session/runner/llm.ts
    pub fn owns(&self, location: &LocationRef) -> bool {
        self.directory == location.directory && self.workspace_id == location.workspace_id
    }
}
