//! Model schema mirror.
//!
//! From reference/packages/schema/src/model.ts and
//! reference/packages/core/src/model.ts.
//!
//! TODO(integration): promote to oc-schema (Model types).

use serde::{Deserialize, Serialize};

use crate::ids::{ModelId, ProviderId, VariantId};

/// `Model.Ref` — `{ id, providerID, variant? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub id: ModelId,
    pub providerID: ProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<VariantId>,
}

/// `Model.Capabilities` — `{ tools, input, output }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub tools: bool,
    pub input: Vec<String>,
    pub output: Vec<String>,
}

/// `Model.Cost` — `{ tier?, input, output, cache: { read, write } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCost {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<CostTier>,
    pub input: f64,
    pub output: f64,
    pub cache: CostCache,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostTier {
    #[serde(rename = "type")]
    pub kind: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostCache {
    pub read: f64,
    pub write: f64,
}

/// `Model.Api` — `{ id } & (AISDK | Native)`, tagged on `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelApi {
    pub id: ModelId,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ModelApi {
    pub fn native(id: ModelId) -> Self {
        ModelApi {
            id,
            kind: "native".to_string(),
            package: None,
            url: None,
            settings: Some(serde_json::Map::new()),
        }
    }
}

/// `Model.Info`.
/// From reference/packages/schema/src/model.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: ModelId,
    pub providerID: ProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub name: String,
    pub api: ModelApi,
    pub capabilities: ModelCapabilities,
    pub request: ModelRequest,
    pub variants: Vec<ModelVariant>,
    pub time: ModelTime,
    pub cost: Vec<ModelCost>,
    pub status: String,
    pub enabled: bool,
    pub limit: ModelLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub headers: serde_json::Map<String, serde_json::Value>,
    pub body: serde_json::Map<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelVariant {
    pub id: VariantId,
    pub headers: serde_json::Map<String, serde_json::Value>,
    pub body: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTime {
    pub released: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimit {
    pub context: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    pub output: i64,
}

impl ModelInfo {
    /// Mirrors `Model.Info.empty(providerID, modelID)`.
    pub fn empty(provider_id: ProviderId, model_id: ModelId) -> Self {
        ModelInfo {
            id: model_id.clone(),
            providerID: provider_id,
            family: None,
            name: model_id.0.clone(),
            api: ModelApi::native(model_id),
            capabilities: ModelCapabilities {
                tools: false,
                input: Vec::new(),
                output: Vec::new(),
            },
            request: ModelRequest {
                headers: serde_json::Map::new(),
                body: serde_json::Map::new(),
                variant: None,
            },
            variants: Vec::new(),
            time: ModelTime { released: 0.0 },
            cost: Vec::new(),
            status: "active".to_string(),
            enabled: true,
            limit: ModelLimit {
                context: 0,
                input: None,
                output: 0,
            },
        }
    }
}
