//! From reference/packages/schema/src/connection.ts

use crate::credential;
use serde::{Deserialize, Serialize};

/// `Connection.CredentialInfo`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CredentialInfo {
    #[serde(rename = "type")]
    pub r#type: CredentialInfoType,
    pub id: credential::ID,
    pub label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CredentialInfoType {
    #[serde(rename = "credential")]
    Value,
}

/// `Connection.EnvInfo`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EnvInfo {
    #[serde(rename = "type")]
    pub r#type: EnvInfoType,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum EnvInfoType {
    #[serde(rename = "env")]
    Value,
}

/// `Connection.Info` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Info {
    Credential(CredentialInfo),
    Env(EnvInfo),
}
