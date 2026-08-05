// From reference/packages/core/src/config/mcp.ts

use crate::jsnum::PositiveInt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timeout {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<PositiveInt>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Local {
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Timeout>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::jsnum::de_port_opt"
    )]
    pub callback_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Remote {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Timeout>,
}

/// `Schema.Union([OAuth, Schema.Literal(false)])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OAuthValue {
    Off(bool),
    OAuth(OAuth),
}

/// `Server` = `Union([Local, Remote])` tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Server {
    Local(Local),
    Remote(Remote),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Timeout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<IndexMap<String, Server>>,
}
