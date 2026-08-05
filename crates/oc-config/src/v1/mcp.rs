// From reference/packages/core/src/v1/config/mcp.ts

use crate::jsnum::PositiveInt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// `ConfigMCPV1.Info` discriminated by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Info {
    Local(Local),
    Remote(Remote),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Local {
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<PositiveInt>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Remote {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<PositiveInt>,
}

/// `Schema.Union([OAuth, Schema.Literal(false)])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OAuthValue {
    Off(bool),
    OAuth(OAuth),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clientId: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clientSecret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::jsnum::de_port_opt"
    )]
    pub callbackPort: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirectUri: Option<String>,
}

/// A bare `{ enabled: boolean }` entry — the third member of the value union
/// in `ConfigV1.Info.mcp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Enabled {
    pub enabled: bool,
}

/// One `mcp` map entry: a local/remote server or a bare `{ enabled }` flag.
/// The large `Server` payload is boxed (serde-transparent) to keep the enum
/// size small (clippy `large_enum_variant`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Server(Box<Info>),
    Enabled(Enabled),
}
