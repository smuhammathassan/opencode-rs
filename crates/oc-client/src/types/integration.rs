//! Integration types.
//! From reference/packages/schema/src/integration.ts.

use crate::types::connection::ConnectionInfo;
use crate::types::location::LocationQueryRef;
use crate::types::schema::JsonValue;

/// `Integration.Ref`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationRef {
    pub id: String,
    pub name: String,
}

/// `Integration.When`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationWhen {
    pub key: String,
    pub op: IntegrationWhenOp,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationWhenOp {
    Eq,
    Neq,
}

/// `Integration.Prompt` — tagged on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum IntegrationPrompt {
    #[serde(rename = "text")]
    Text {
        key: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<IntegrationWhen>,
    },
    #[serde(rename = "select")]
    Select {
        key: String,
        message: String,
        options: Vec<IntegrationOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<IntegrationWhen>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationOption {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `Integration.Method` — tagged on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum IntegrationMethod {
    #[serde(rename = "oauth")]
    Oauth {
        id: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompts: Option<Vec<IntegrationPrompt>>,
    },
    #[serde(rename = "key")]
    Key {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    #[serde(rename = "env")]
    Env { names: Vec<String> },
}

/// `Integration.Inputs` — a `String -> String` map.
pub type IntegrationInputs = std::collections::HashMap<String, String>;

/// `Integration.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationInfo {
    pub id: String,
    pub name: String,
    pub methods: Vec<IntegrationMethod>,
    pub connections: Vec<ConnectionInfo>,
}

/// `Integration.Attempt` — the response of `integration.connect.oauth`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationAttempt {
    #[serde(rename = "attemptID")]
    pub attempt_id: String,
    pub url: String,
    pub instructions: String,
    pub mode: IntegrationAttemptMode,
    pub time: IntegrationAttemptTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationAttemptMode {
    Auto,
    Code,
}

/// `Schema.Number` encodes `Infinity`/`-Infinity`/`NaN` as strings, so these
/// fields are kept as raw JSON.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationAttemptTime {
    pub created: JsonValue,
    pub expires: JsonValue,
}

/// `Integration.AttemptStatus` — tagged on `status`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "status")]
pub enum IntegrationAttemptStatus {
    #[serde(rename = "pending")]
    Pending { time: IntegrationAttemptTime },
    #[serde(rename = "complete")]
    Complete { time: IntegrationAttemptTime },
    #[serde(rename = "failed")]
    Failed {
        message: String,
        time: IntegrationAttemptTime,
    },
    #[serde(rename = "expired")]
    Expired { time: IntegrationAttemptTime },
}

/// `IntegrationsGetInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsGetInput {
    pub integration_id: String,
    pub location: Option<LocationQueryRef>,
}

/// `IntegrationsConnectKeyInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsConnectKeyInput {
    pub integration_id: String,
    pub location: Option<LocationQueryRef>,
    pub key: String,
    pub label: Option<String>,
}

/// `IntegrationsConnectOauthInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsConnectOauthInput {
    pub integration_id: String,
    pub location: Option<LocationQueryRef>,
    pub method_id: String,
    pub inputs: IntegrationInputs,
    pub label: Option<String>,
}

/// `IntegrationsAttemptStatusInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsAttemptStatusInput {
    pub attempt_id: String,
    pub location: Option<LocationQueryRef>,
}

/// `IntegrationsAttemptCompleteInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsAttemptCompleteInput {
    pub attempt_id: String,
    pub location: Option<LocationQueryRef>,
    pub code: Option<String>,
}

/// `IntegrationsAttemptCancelInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsAttemptCancelInput {
    pub attempt_id: String,
    pub location: Option<LocationQueryRef>,
}
