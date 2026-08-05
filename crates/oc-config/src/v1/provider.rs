// From reference/packages/core/src/v1/config/provider.ts

use crate::jsnum::{de_f64, de_f64_opt, serialize_js_number, serialize_js_number_opt, PositiveInt};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `ModelStatus` = `Schema.Literals(["alpha", "beta", "deprecated", "active"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Alpha,
    Beta,
    Deprecated,
    Active,
}

/// `Modality` = `Schema.Literals(["text", "audio", "image", "video", "pdf"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Audio,
    Image,
    Video,
    Pdf,
}

/// `interleaved` = `Union([Boolean, InterleavedField, { field }])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Interleaved {
    Bool(bool),
    Field(String),
    Wrapped { field: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCostContext {
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub input: f64,
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub output: f64,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "de_f64_opt", serialize_with = "serialize_js_number_opt")]
    pub cache_read: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "de_f64_opt", serialize_with = "serialize_js_number_opt")]
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCost {
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub input: f64,
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub output: f64,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "de_f64_opt", serialize_with = "serialize_js_number_opt")]
    pub cache_read: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "de_f64_opt", serialize_with = "serialize_js_number_opt")]
    pub cache_write: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_over_200k: Option<ModelCostContext>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelLimit {
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub context: f64,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "de_f64_opt", serialize_with = "serialize_js_number_opt")]
    pub input: Option<f64>,
    #[serde(deserialize_with = "de_f64", serialize_with = "serialize_js_number")]
    pub output: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Modalities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<Modality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Vec<Modality>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelProvider {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
}

/// A model variant override: `{ disabled?: boolean, ...rest }`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelVariant {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(flatten)]
    pub rest: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interleaved: Option<Interleaved>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelCost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<ModelLimit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Modalities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ModelStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ModelProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<IndexMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<IndexMap<String, ModelVariant>>,
}

/// `Schema.Union([PositiveInt, Schema.Literal(false)])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Timeout {
    Ms(PositiveInt),
    Off(bool),
}

/// Provider `options` with known fields plus an open-ended rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Options {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apiKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseURL: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterpriseUrl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setCacheKey: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Timeout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headerTimeout: Option<Timeout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunkTimeout: Option<PositiveInt>,
    #[serde(flatten)]
    pub rest: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whitelist: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blacklist: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<IndexMap<String, Model>>,
}
