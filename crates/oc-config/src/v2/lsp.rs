// From reference/packages/core/src/config/lsp.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `{ disabled: true }` marker. `false` must fall through to `Server`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Disabled {
    pub disabled: bool,
}

impl<'de> Deserialize<'de> for Disabled {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = bool::deserialize(deserializer)?;
        if value {
            Ok(Disabled { disabled: true })
        } else {
            Err(serde::de::Error::custom("disabled must be true"))
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization: Option<IndexMap<String, Value>>,
}

/// `Entry` = `Union([Disabled, Server])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Disabled(Disabled),
    Server(Server),
}

/// `Info` = `Union([Boolean, Record<String, Entry>])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Info {
    Bool(bool),
    ByLanguage(IndexMap<String, Entry>),
}
