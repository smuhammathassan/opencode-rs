//! From reference/packages/schema/src/provider.ts

use crate::integration_id::IntegrationID;
use crate::schema::Json;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `ProviderV2.ID`.
pub type ID = String;

/// Known provider IDs from the `statics` on `Provider.ID`.
pub const OPENCODE: &str = "opencode";
pub const ANTHROPIC: &str = "anthropic";
pub const OPENAI: &str = "openai";
pub const GOOGLE: &str = "google";
pub const GOOGLE_VERTEX: &str = "google-vertex";
pub const GITHUB_COPILOT: &str = "github-copilot";
pub const AMAZON_BEDROCK: &str = "amazon-bedrock";
pub const AZURE: &str = "azure";
pub const OPENROUTER: &str = "openrouter";
pub const MISTRAL: &str = "mistral";
pub const GITLAB: &str = "gitlab";

/// `Provider.AISDK`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AISDK {
    #[serde(rename = "type")]
    pub r#type: AISDKType,
    pub package: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub settings: Option<IndexMap<String, Value>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AISDKType {
    #[serde(rename = "aisdk")]
    Value,
}

/// `Provider.Native`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Native {
    #[serde(rename = "type")]
    pub r#type: NativeType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    pub settings: IndexMap<String, Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NativeType {
    #[serde(rename = "native")]
    Value,
}

/// `Provider.Api` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Api {
    Aisdk(AISDK),
    Native(Native),
}

/// `Provider.Request`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Request {
    pub headers: IndexMap<String, String>,
    pub body: IndexMap<String, Json>,
}

/// `ProviderV2.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    #[serde(rename = "integrationID", skip_serializing_if = "Option::is_none", default)]
    pub integration_id: Option<IntegrationID>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub disabled: Option<bool>,
    pub api: Api,
    pub request: Request,
}

/// `Provider.empty(id)`.
pub fn empty(id: ID) -> Info {
    Info {
        id: id.clone(),
        integration_id: None,
        name: id,
        disabled: None,
        api: Api::Native(Native {
            r#type: NativeType::Value,
            url: None,
            settings: IndexMap::new(),
        }),
        request: Request {
            headers: IndexMap::new(),
            body: IndexMap::new(),
        },
    }
}
