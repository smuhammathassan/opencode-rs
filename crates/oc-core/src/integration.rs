//! Integration schema mirror and a minimal service.
//!
//! From reference/packages/schema/src/integration.ts and
//! reference/packages/core/src/integration.ts.
//!
//! The full integration runtime (OAuth flows, attempts) is owned by
//! oc-provider. oc-core's catalog needs only `list()`; this service is a
//! registry that a provider-owned integration layer can populate.
//! TODO(integration): replace with the oc-provider integration service.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::ids::IntegrationId;

/// `Integration.When` — `{ key, op, value }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct When {
    pub key: String,
    pub op: String,
    pub value: String,
}

/// `Integration.TextPrompt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPrompt {
    #[serde(rename = "type")]
    pub kind: String,
    pub key: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

/// `Integration.SelectPrompt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectPrompt {
    #[serde(rename = "type")]
    pub kind: String,
    pub key: String,
    pub message: String,
    pub options: Vec<SelectOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `Integration.Prompt` — tagged union on `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Prompt {
    #[serde(rename = "text")]
    Text(TextPrompt),
    #[serde(rename = "select")]
    Select(SelectPrompt),
}

/// `Integration.Method` — tagged union on `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Method {
    #[serde(rename = "oauth")]
    OAuth(OAuthMethod),
    #[serde(rename = "key")]
    Key(KeyMethod),
    #[serde(rename = "env")]
    Env(EnvMethod),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthMethod {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<Prompt>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMethod {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvMethod {
    pub names: Vec<String>,
}

/// `Connection.Info` — tagged union on `type`.
/// From reference/packages/schema/src/connection.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConnectionInfo {
    #[serde(rename = "credential")]
    Credential(CredentialConnection),
    #[serde(rename = "env")]
    Env(EnvConnection),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialConnection {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvConnection {
    pub name: String,
}

/// `Integration.Info` — `{ id, name, methods, connections }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationInfo {
    pub id: IntegrationId,
    pub name: String,
    pub methods: Vec<Method>,
    pub connections: Vec<ConnectionInfo>,
}

/// Minimal integration service for catalog availability checks.
#[derive(Clone, Default)]
pub struct IntegrationService {
    infos: Arc<Mutex<Vec<IntegrationInfo>>>,
}

impl IntegrationService {
    pub fn new() -> Self {
        IntegrationService::default()
    }

    /// Populate the registry (the provider-owned integration layer feeds this).
    pub fn set(&self, infos: Vec<IntegrationInfo>) {
        *self.infos.lock().unwrap() = infos;
    }

    /// `Integration.list()`.
    pub async fn list(&self) -> Vec<IntegrationInfo> {
        self.infos.lock().unwrap().clone()
    }
}
