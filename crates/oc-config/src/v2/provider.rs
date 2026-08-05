// From reference/packages/core/src/config/provider.ts

use crate::jsnum::{de_f64, de_f64_opt, serialize_js_number, serialize_js_number_opt};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `ProviderV2.Request` — header/body overrides.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<IndexMap<String, Value>>,
}

/// `ConfigV2.Model.Cost.Cache`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "de_f64_opt", serialize_with = "serialize_js_number_opt")]
    pub read: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "de_f64_opt", serialize_with = "serialize_js_number_opt")]
    pub write: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostTier {
    #[serde(rename = "type")]
    pub kind: TierKind,
    pub size: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TierKind {
    Context,
}

/// `ConfigV2.Model.Cost`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<CostTier>,
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub input: f64,
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub output: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<Cache>,
}

/// `ConfigV2.Model.Limit`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<i64>,
}

/// `ModelV2.Capabilities`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// A model override request — `Request` fields plus an optional `variant`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<IndexMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// A variant override — `Request` fields keyed by a variant id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelVariant {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<IndexMap<String, Value>>,
}

/// `Model.api` — `ModelV2.Api` union (AISDK / Native / bare id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelApi {
    Tagged(ModelApiTagged),
    Id { id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ModelApiTagged {
    Aisdk {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        package: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        settings: Option<IndexMap<String, Value>>,
    },
    Native {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default)]
        settings: IndexMap<String, Value>,
    },
}

/// `cost` = `Union([Cost, Array<Cost>])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CostOrArray {
    Cost(Cost),
    Array(Vec<Cost>),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    /// `ModelV2.Family` (branded string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<ModelApi>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<ModelRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<ModelVariant>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostOrArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<Limit>,
}

/// `ConfigProvider.Info` — provider overrides authored in opencode.json.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<ProviderApi>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Request>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<IndexMap<String, Model>>,
}

/// `ProviderV2.Api` — AISDK / Native tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProviderApi {
    Aisdk {
        package: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        settings: Option<IndexMap<String, Value>>,
    },
    Native {
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default)]
        settings: IndexMap<String, Value>,
    },
}
