//! Credential types.
//! From reference/packages/schema/src/credential.ts.

// TODO(integration): promote to oc-schema.
use crate::types::location::LocationQueryRef;
use crate::types::schema::JsonValue;
use std::collections::HashMap;

/// `Credential.Value` — tagged on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum CredentialValue {
    #[serde(rename = "oauth")]
    Oauth {
        method_id: String,
        refresh: String,
        access: String,
        expires: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<HashMap<String, JsonValue>>,
    },
    #[serde(rename = "key")]
    Key {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<HashMap<String, JsonValue>>,
    },
}

/// `CredentialsUpdateInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CredentialsUpdateInput {
    pub credential_id: String,
    pub location: Option<LocationQueryRef>,
    pub label: String,
}

/// `CredentialsRemoveInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CredentialsRemoveInput {
    pub credential_id: String,
    pub location: Option<LocationQueryRef>,
}
