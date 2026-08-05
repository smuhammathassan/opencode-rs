//! From reference/packages/schema/src/credential.ts

use crate::identifier::ascending;
use crate::integration_id::IntegrationMethodID;
use crate::schema::NonNegativeInt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// `Credential.ID`.
pub type ID = String;

/// `Credential.ID.create()`.
pub fn create_id() -> ID {
    format!("cred_{}", ascending())
}

/// `Credential.OAuth`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OAuth {
    #[serde(rename = "type")]
    pub r#type: OAuthType,
    #[serde(rename = "methodID")]
    pub method_id: IntegrationMethodID,
    pub refresh: String,
    pub access: String,
    pub expires: NonNegativeInt,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum OAuthType {
    #[serde(rename = "oauth")]
    Value,
}

/// `Credential.Key`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Key {
    #[serde(rename = "type")]
    pub r#type: KeyType,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum KeyType {
    #[serde(rename = "key")]
    Value,
}

/// `Credential.Value` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Value {
    OAuth(OAuth),
    Key(Key),
}
