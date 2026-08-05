//! From reference/packages/schema/src/integration.ts

use crate::connection;
use crate::define_event;
use crate::identifier::ascending;
use crate::integration_id::{IntegrationID, IntegrationMethodID};
use crate::schema::Finite;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// `Integration.ID`.
pub type ID = IntegrationID;

/// `Integration.MethodID`.
pub type MethodID = IntegrationMethodID;

/// `Integration.When.op`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum WhenOp {
    #[serde(rename = "eq")]
    Eq,
    #[serde(rename = "neq")]
    Neq,
}

/// `Integration.When`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct When {
    pub key: String,
    pub op: WhenOp,
    pub value: String,
}

/// `Integration.TextPrompt`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextPrompt {
    #[serde(rename = "type")]
    pub r#type: TextPromptType,
    pub key: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub when: Option<When>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TextPromptType {
    #[serde(rename = "text")]
    Value,
}

/// `Integration.SelectPrompt` option.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint: Option<String>,
}

/// `Integration.SelectPrompt`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SelectPrompt {
    #[serde(rename = "type")]
    pub r#type: SelectPromptType,
    pub key: String,
    pub message: String,
    pub options: Vec<SelectOption>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub when: Option<When>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SelectPromptType {
    #[serde(rename = "select")]
    Value,
}

/// `Integration.Prompt` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Prompt {
    Text(TextPrompt),
    Select(SelectPrompt),
}

/// `Integration.OAuthMethod`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OAuthMethod {
    pub id: MethodID,
    #[serde(rename = "type")]
    pub r#type: OAuthMethodType,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prompts: Option<Vec<Prompt>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum OAuthMethodType {
    #[serde(rename = "oauth")]
    Value,
}

/// `Integration.KeyMethod`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct KeyMethod {
    #[serde(rename = "type")]
    pub r#type: KeyMethodType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum KeyMethodType {
    #[serde(rename = "key")]
    Value,
}

/// `Integration.EnvMethod`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EnvMethod {
    #[serde(rename = "type")]
    pub r#type: EnvMethodType,
    pub names: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum EnvMethodType {
    #[serde(rename = "env")]
    Value,
}

/// `Integration.Method` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Method {
    OAuth(OAuthMethod),
    Key(KeyMethod),
    Env(EnvMethod),
}

/// `Integration.Inputs`.
pub type Inputs = IndexMap<String, String>;

define_event! {
    /// `integration.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "integration.updated",
        data: crate::schema::Empty,
    }
}

define_event! {
    /// `integration.connection.updated`.
    pub struct ConnectionUpdated {
        tag: ConnectionUpdatedTag,
        r#type: "integration.connection.updated",
        data: ConnectionUpdatedData,
    }
}

/// Payload of `integration.connection.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ConnectionUpdatedData {
    #[serde(rename = "integrationID")]
    pub integration_id: ID,
}

/// `Integration.Ref`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Ref {
    pub id: ID,
    pub name: String,
}

/// `Integration.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    pub name: String,
    pub methods: Vec<Method>,
    pub connections: Vec<connection::Info>,
}

/// `Integration.AttemptID`.
pub type AttemptID = String;

/// `Integration.AttemptID.create()`.
pub fn create_attempt_id() -> AttemptID {
    format!("con_{}", ascending())
}

/// `Integration.Attempt.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AttemptTime {
    pub created: Finite,
    pub expires: Finite,
}

/// `Integration.Attempt.mode`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AttemptMode {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "code")]
    Code,
}

/// `Integration.Attempt`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Attempt {
    #[serde(rename = "attemptID")]
    pub attempt_id: AttemptID,
    pub url: String,
    pub instructions: String,
    pub mode: AttemptMode,
    pub time: AttemptTime,
}

/// `Integration.AttemptStatus` — tagged union on `status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum AttemptStatus {
    Pending(AttemptStatusPending),
    Complete(AttemptStatusComplete),
    Failed(AttemptStatusFailed),
    Expired(AttemptStatusExpired),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AttemptStatusPending {
    pub status: AttemptStatusPendingStatus,
    pub time: AttemptTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AttemptStatusPendingStatus {
    #[serde(rename = "pending")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AttemptStatusComplete {
    pub status: AttemptStatusCompleteStatus,
    pub time: AttemptTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AttemptStatusCompleteStatus {
    #[serde(rename = "complete")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AttemptStatusFailed {
    pub status: AttemptStatusFailedStatus,
    pub message: String,
    pub time: AttemptTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AttemptStatusFailedStatus {
    #[serde(rename = "failed")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AttemptStatusExpired {
    pub status: AttemptStatusExpiredStatus,
    pub time: AttemptTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AttemptStatusExpiredStatus {
    #[serde(rename = "expired")]
    Value,
}

/// `Integration.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::{ConnectionUpdated, Updated};
    pub use crate::event::Definition;

    /// `Integration.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[
        Definition {
            r#type: "integration.updated",
            durable: None,
        },
        Definition {
            r#type: "integration.connection.updated",
            durable: None,
        },
    ];
}

/// `Integration.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
