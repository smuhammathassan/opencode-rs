//! Model types.
//! From reference/packages/schema/src/model.ts.

use crate::types::schema::JsonValue;
use crate::types::session_message::TokenCache;
use std::collections::HashMap;

/// `Model.Ref`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// `Model.Api` — tagged union on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ModelApi {
    #[serde(rename = "aisdk")]
    Aisdk {
        id: String,
        package: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        settings: Option<HashMap<String, JsonValue>>,
    },
    #[serde(rename = "native")]
    Native {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        settings: HashMap<String, JsonValue>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    pub tools: bool,
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    pub headers: HashMap<String, String>,
    pub body: HashMap<String, JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVariant {
    pub id: String,
    pub headers: HashMap<String, String>,
    pub body: HashMap<String, JsonValue>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTier {
    #[serde(rename = "type")]
    pub kind: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
    pub input: f64,
    pub output: f64,
    pub cache: TokenCache,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Alpha,
    Beta,
    Deprecated,
    Active,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLimit {
    pub context: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    pub output: i64,
}

/// `Model.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub provider_id: String,
    #[serde(default)]
    pub family: Option<String>,
    pub name: String,
    pub api: ModelApi,
    pub capabilities: ModelCapabilities,
    pub request: ModelRequest,
    pub variants: Vec<ModelVariant>,
    pub time: ModelTime,
    pub cost: Vec<ModelCost>,
    pub status: ModelStatus,
    pub enabled: bool,
    pub limit: ModelLimit,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTime {
    pub released: f64,
}
