//! Models.dev catalog data types and the embedded registry snapshot.
//!
//! From reference/packages/core/src/models-dev.ts and
//! reference/packages/schema/src/models-dev.ts.
//!
//! The reference fetches the catalog from `https://models.opencode.ai/api.json`
//! at runtime (cached under `Global.Path.cache/models.json`, 5-minute TTL) and
//! falls back to a build-time snapshot injected as `OPENCODE_MODELS_DEV`. The
//! Rust port embeds that snapshot as `data/models.json` (regenerated from the
//! same URL) so the registry works offline and is deterministically testable.
//!
//! TODO(integration): regenerate `data/models.json` from
//! `https://models.opencode.ai/api.json` when packaging a release so the
//! embedded catalog matches the reference build snapshot. The runtime refresh
//! flow (fetch + TTL cache + flock) is not ported here.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::provider::model_status::CatalogModelStatus;

/// A modality a model can accept as input or produce as output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Audio,
    Image,
    Video,
    Pdf,
}

impl Modality {
    pub fn as_str(self) -> &'static str {
        match self {
            Modality::Text => "text",
            Modality::Audio => "audio",
            Modality::Image => "image",
            Modality::Video => "video",
            Modality::Pdf => "pdf",
        }
    }

    pub fn from_str(s: &str) -> Option<Modality> {
        match s {
            "text" => Some(Modality::Text),
            "audio" => Some(Modality::Audio),
            "image" => Some(Modality::Image),
            "video" => Some(Modality::Video),
            "pdf" => Some(Modality::Pdf),
            _ => None,
        }
    }
}

/// `models_dev` interleaved reasoning field: `boolean | string | { field }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Interleaved {
    Bool(bool),
    Field(String),
    Struct { field: String },
}

impl<'de> Deserialize<'de> for Interleaved {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Bool(b) => Ok(Interleaved::Bool(b)),
            serde_json::Value::String(s) => Ok(Interleaved::Field(s)),
            serde_json::Value::Object(mut o) => {
                let field = o
                    .remove("field")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => Some(s),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        serde::de::Error::custom("interleaved object missing `field`")
                    })?;
                Ok(Interleaved::Struct { field })
            }
            _ => Err(serde::de::Error::custom("invalid interleaved value")),
        }
    }
}

/// One entry of `reasoning_options`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReasoningOption {
    Effort {
        #[serde(deserialize_with = "deserialize_nullable_strings")]
        values: Vec<Option<String>>,
    },
    Toggle,
    BudgetTokens {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
}

fn deserialize_nullable_strings<'de, D>(deserializer: D) -> Result<Vec<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .map(|v| match v {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s),
            _ => None,
        })
        .collect())
}

/// Tiered pricing entry within `cost`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CostTier {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    pub tier: Tier,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tier {
    #[serde(rename = "type")]
    pub r#type: String,
    pub size: f64,
}

/// Pricing for a model. Fields are tolerant (`Option`) because the catalog is
/// a trusted upstream dataset and `fromModelsDevModel` supplies defaults.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Cost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    #[serde(default)]
    pub tiers: Option<Vec<CostTier>>,
    #[serde(default)]
    pub context_over_200k: Option<ContextOver200K>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ContextOver200K {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// Context / output token limits.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Limit {
    #[serde(default)]
    pub context: Option<f64>,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
}

/// Input/output modalities.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<Modality>,
    #[serde(default)]
    pub output: Vec<Modality>,
}

/// Optional model-level npm/api override.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderRef {
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
}

/// A single experimental mode for a model (`experimental.modes`).
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Mode {
    #[serde(default)]
    pub cost: Option<Cost>,
    #[serde(default)]
    pub provider: Option<ModeProvider>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModeProvider {
    #[serde(default)]
    pub body: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Experimental {
    #[serde(default)]
    pub modes: Option<IndexMap<String, Mode>>,
}

/// A model as advertised by the models.dev catalog.
///
/// Mirrors `ModelsDev.Model` in `models-dev.ts`. Only the fields consumed by
/// `fromModelsDevModel` are typed; the rest of the upstream JSON is dropped,
/// matching Effect's `Schema.Struct` which discards unknown keys.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Model {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub attachment: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub reasoning_options: Option<Vec<ReasoningOption>>,
    #[serde(default)]
    pub interleaved: Option<Interleaved>,
    #[serde(default)]
    pub cost: Option<Cost>,
    #[serde(default)]
    pub limit: Option<Limit>,
    #[serde(default)]
    pub modalities: Option<Modalities>,
    #[serde(default)]
    pub experimental: Option<Experimental>,
    #[serde(default)]
    pub status: Option<CatalogModelStatus>,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
}

/// A provider as advertised by the models.dev catalog.
///
/// Mirrors `ModelsDev.Provider` in `models-dev.ts`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Provider {
    #[serde(default)]
    pub api: Option<String>,
    pub name: String,
    #[serde(default)]
    pub env: Vec<String>,
    pub id: String,
    #[serde(default)]
    pub npm: Option<String>,
    pub models: IndexMap<String, Model>,
}

/// The embedded catalog snapshot: `Record<ProviderID, Provider>`.
pub const MODELS_JSON: &str = include_str!("../data/models.json");

/// Parses the embedded catalog snapshot.
pub fn snapshot() -> Result<IndexMap<String, Provider>, serde_json::Error> {
    serde_json::from_str(MODELS_JSON)
}
