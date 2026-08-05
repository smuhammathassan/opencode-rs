//! Branded ID newtypes shared across oc-core modules.
//!
//! Each mirrors a `Schema.brand(...)` from the reference schema package.
//! They serialize transparently as plain JSON strings.
//!
//! TODO(integration): promote to oc-schema when that crate is populated.

use serde::{Deserialize, Serialize};

use crate::identifier;

/// `Event.ID` — starts with `evt_`.
/// From reference/packages/schema/src/event.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(pub String);

impl EventId {
    /// Mirrors `Event.ID.create()` → `"evt_" + ascending()`.
    pub fn create() -> Self {
        EventId(format!("evt_{}", identifier::ascending()))
    }
}

impl From<&str> for EventId {
    fn from(value: &str) -> Self {
        EventId(value.to_string())
    }
}

impl From<String> for EventId {
    fn from(value: String) -> Self {
        EventId(value)
    }
}

/// `Credential.ID` — starts with `cred_`.
/// From reference/packages/schema/src/credential.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialId(pub String);

impl CredentialId {
    /// Mirrors `Credential.ID.create()` → `"cred_" + ascending()`.
    pub fn create() -> Self {
        CredentialId(format!("cred_{}", identifier::ascending()))
    }
}

impl From<&str> for CredentialId {
    fn from(value: &str) -> Self {
        CredentialId(value.to_string())
    }
}

/// `WorkspaceV2.ID` — starts with `wrk`.
/// From reference/packages/schema/src/workspace-id.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub String);

impl WorkspaceId {
    /// Mirrors `WorkspaceID.ascending(id?)`.
    pub fn ascending(id: Option<&str>) -> Result<Self, String> {
        match id {
            Some(value) if !value.starts_with("wrk") => {
                Err(format!("ID {value} does not start with wrk"))
            }
            Some(value) => Ok(WorkspaceId(value.to_string())),
            None => Ok(WorkspaceId(format!("wrk_{}", identifier::ascending()))),
        }
    }
}

/// `Project.ID` — branded string, `global` sentinel.
/// From reference/packages/schema/src/project-id.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub String);

impl ProjectId {
    /// Mirrors `ProjectID.global`.
    pub fn global() -> Self {
        ProjectId("global".to_string())
    }

    /// Mirrors `ProjectID.make(...)`.
    pub fn make(value: impl Into<String>) -> Self {
        ProjectId(value.into())
    }
}

impl From<&str> for ProjectId {
    fn from(value: &str) -> Self {
        ProjectId(value.to_string())
    }
}

impl From<String> for ProjectId {
    fn from(value: String) -> Self {
        ProjectId(value)
    }
}

/// `AgentV2.ID` — branded string.
/// From reference/packages/schema/src/agent.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(pub String);

impl AgentId {
    /// Mirrors `Agent.ID.make(...)`.
    pub fn make(value: impl Into<String>) -> Self {
        AgentId(value.into())
    }
}

impl From<&str> for AgentId {
    fn from(value: &str) -> Self {
        AgentId(value.to_string())
    }
}

/// `Integration.ID` — branded string.
/// From reference/packages/schema/src/integration-id.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntegrationId(pub String);

impl IntegrationId {
    pub fn make(value: impl Into<String>) -> Self {
        IntegrationId(value.into())
    }
}

impl From<&str> for IntegrationId {
    fn from(value: &str) -> Self {
        IntegrationId(value.to_string())
    }
}

/// `Integration.MethodID` — branded string.
/// From reference/packages/schema/src/integration-id.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntegrationMethodId(pub String);

/// `ProviderV2.ID` — branded string with well-known constants.
/// From reference/packages/schema/src/provider.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn opencode() -> Self {
        ProviderId("opencode".to_string())
    }
    pub fn anthropic() -> Self {
        ProviderId("anthropic".to_string())
    }
    pub fn openai() -> Self {
        ProviderId("openai".to_string())
    }
    pub fn google() -> Self {
        ProviderId("google".to_string())
    }
    pub fn google_vertex() -> Self {
        ProviderId("google-vertex".to_string())
    }
    pub fn github_copilot() -> Self {
        ProviderId("github-copilot".to_string())
    }
    pub fn amazon_bedrock() -> Self {
        ProviderId("amazon-bedrock".to_string())
    }
    pub fn azure() -> Self {
        ProviderId("azure".to_string())
    }
    pub fn openrouter() -> Self {
        ProviderId("openrouter".to_string())
    }
    pub fn mistral() -> Self {
        ProviderId("mistral".to_string())
    }
    pub fn gitlab() -> Self {
        ProviderId("gitlab".to_string())
    }

    /// Mirrors `Provider.ID.make(...)`.
    pub fn make(value: impl Into<String>) -> Self {
        ProviderId(value.into())
    }
}

impl From<&str> for ProviderId {
    fn from(value: &str) -> Self {
        ProviderId(value.to_string())
    }
}

impl From<String> for ProviderId {
    fn from(value: String) -> Self {
        ProviderId(value)
    }
}

/// `ModelV2.ID` — branded string.
/// From reference/packages/schema/src/model.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub String);

impl ModelId {
    /// Mirrors `Model.ID.make(...)`.
    pub fn make(value: impl Into<String>) -> Self {
        ModelId(value.into())
    }
}

impl From<&str> for ModelId {
    fn from(value: &str) -> Self {
        ModelId(value.to_string())
    }
}

/// `ModelV2.VariantID` — branded string.
/// From reference/packages/schema/src/model.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VariantId(pub String);

/// `AccountV2.ID` — branded string.
/// From reference/packages/core/src/account.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub String);

impl From<&str> for AccountId {
    fn from(value: &str) -> Self {
        AccountId(value.to_string())
    }
}

/// `OrgID` — branded string.
/// From reference/packages/core/src/account.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrgId(pub String);

impl From<&str> for OrgId {
    fn from(value: &str) -> Self {
        OrgId(value.to_string())
    }
}

/// `SessionV1.ID` / `SessionMessage.ID` — needed by permission schemas.
/// From reference/packages/schema/src/session-id.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

impl From<&str> for SessionId {
    fn from(value: &str) -> Self {
        SessionId(value.to_string())
    }
}
