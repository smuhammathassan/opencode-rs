/// Local mirror of `reference/packages/opencode/src/provider/provider.ts`
/// `Model` schema plus `reference/packages/opencode/src/provider/transform.ts`
/// helpers that oc-session needs.
///
/// TODO(integration): promote to oc-provider once that crate implements the
/// full Provider.Model.
use serde::{Deserialize, Serialize};

use crate::JsonMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCost {
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostBase {
    pub input: f64,
    pub output: f64,
    pub cache: CacheCost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostTier {
    pub input: f64,
    pub output: f64,
    pub cache: CacheCost,
    pub tier: Tier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tier {
    pub type_: String,
    pub size: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCost {
    pub input: f64,
    pub output: f64,
    pub cache: CacheCost,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<CostTier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental_over_200_k: Option<CostBase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLimit {
    pub context: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    pub output: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiInfo {
    pub id: String,
    pub npm: Option<String>,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub id: String,
    pub provider_id: String,
    pub api: ProviderApiInfo,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub capabilities: JsonMap,
    pub cost: ProviderCost,
    pub limit: ProviderLimit,
    pub status: String,
    pub options: JsonMap,
    pub headers: JsonMap,
    pub release_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<JsonMap>,
}

impl ProviderModel {
    pub fn empty(id: &str, provider_id: &str) -> Self {
        ProviderModel {
            id: id.to_string(),
            provider_id: provider_id.to_string(),
            api: ProviderApiInfo {
                id: id.to_string(),
                npm: None,
                type_: "native".to_string(),
            },
            name: id.to_string(),
            family: None,
            capabilities: JsonMap::new(),
            cost: ProviderCost::default(),
            limit: ProviderLimit {
                context: 0.0,
                input: None,
                output: 0.0,
            },
            status: "active".to_string(),
            options: JsonMap::new(),
            headers: JsonMap::new(),
            release_date: String::new(),
            variants: None,
        }
    }
}

/// From reference `packages/opencode/src/provider/transform.ts`
pub mod transform {
    use super::*;

    /// From reference `transform.ts:maxOutputTokens`.
    pub fn max_output_tokens(model: &ProviderModel, output_token_max: Option<u64>) -> f64 {
        match output_token_max {
            Some(max) => max as f64,
            None => model.limit.output,
        }
    }
}
