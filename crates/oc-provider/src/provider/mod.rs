//! Provider registry: model/provider data types and the models.dev conversion.
//!
//! From reference/packages/opencode/src/provider/provider.ts.

pub mod auth;
pub mod error;
pub mod model_status;
pub mod transform;

pub mod registry;

pub use model_status::{CatalogModelStatus, ModelStatus};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::models_dev;

/// `Provider.api` info: the wire identifier, base URL and npm package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiInfo {
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub npm: String,
}

/// Modal capabilities of a model (`text`/`audio`/`image`/`video`/`pdf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub struct Modalities {
    pub text: bool,
    pub audio: bool,
    pub image: bool,
    pub video: bool,
    pub pdf: bool,
}

impl Modalities {
    pub fn get(&self, modality: models_dev::Modality) -> bool {
        match modality {
            models_dev::Modality::Text => self.text,
            models_dev::Modality::Audio => self.audio,
            models_dev::Modality::Image => self.image,
            models_dev::Modality::Video => self.video,
            models_dev::Modality::Pdf => self.pdf,
        }
    }
}

/// Interleaved reasoning field: `boolean | { field }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InterleavedField {
    Bool(bool),
    Field { field: String },
}

impl Default for InterleavedField {
    fn default() -> Self {
        InterleavedField::Bool(false)
    }
}

/// Model capabilities as stored in the provider registry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(default)]
    pub temperature: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub attachment: bool,
    #[serde(default)]
    pub toolcall: bool,
    #[serde(default)]
    pub input: Modalities,
    #[serde(default)]
    pub output: Modalities,
    #[serde(default)]
    pub interleaved: InterleavedField,
}

/// Cache read/write pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CacheCost {
    #[serde(default)]
    pub read: f64,
    #[serde(default)]
    pub write: f64,
}

/// A context-size tier within `Cost.tiers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostTier {
    pub input: f64,
    pub output: f64,
    pub cache: CacheCost,
    pub tier: CostTierSize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostTierSize {
    #[serde(rename = "type")]
    pub r#type: String,
    pub size: f64,
}

/// Pricing for a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache: CacheCost,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<CostTier>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "experimentalOver200K"
    )]
    pub experimental_over_200k: Option<ExperimentalOver200K>,
}

/// Pricing for prompts beyond 200k tokens (`experimentalOver200K`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalOver200K {
    pub input: f64,
    pub output: f64,
    pub cache: CacheCost,
}

/// Token limits for a model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    #[serde(default)]
    pub context: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: f64,
}

/// A model in the provider registry.
///
/// Mirrors `Model` in `provider.ts`. `variants` is always present after
/// conversion, mirroring the reference where `fromModelsDevModel` and the
/// config path both assign it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub api: ApiInfo,
    #[serde(default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub cost: Cost,
    #[serde(default)]
    pub limit: Limit,
    #[serde(default)]
    pub status: ModelStatus,
    #[serde(default)]
    pub options: serde_json::Map<String, Value>,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default, rename = "release_date")]
    pub release_date: String,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variants: IndexMap<String, serde_json::Map<String, Value>>,
}

impl Model {
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
}

/// Where a provider's credentials came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Env,
    Config,
    Custom,
    Api,
}

/// A provider in the registry.
///
/// Mirrors `Info` in `provider.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: String,
    pub name: String,
    pub source: Source,
    pub env: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub options: serde_json::Map<String, Value>,
    pub models: IndexMap<String, Model>,
}

impl Info {
    pub fn model(&self, model_id: &str) -> Option<&Model> {
        self.models.get(model_id)
    }
}

/// Result of `Provider.list()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResult {
    pub all: Vec<Info>,
    pub default: IndexMap<String, String>,
    pub connected: Vec<String>,
}

/// Result of the config-providers endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProvidersResult {
    pub providers: Vec<Info>,
    pub default: IndexMap<String, String>,
}

/// `Provider.model_suggestions` uses fuzzysort in the reference. This is a
/// lightweight subsequence matcher with fuzzysort-compatible ordering for the
/// `limit: 3, threshold: -10000` call.
///
/// TODO(integration): replace with a faithful fuzzysort scoring port if exact
/// suggestion parity is required by the CLI surface.
fn fuzzy_match(query: &str, candidates: &[String]) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let query_chars: Vec<char> = query_lower.chars().collect();
    let mut scored: Vec<(i64, &String)> = Vec::new();
    for candidate in candidates {
        let mut qi = 0usize;
        let mut score: i64 = 0;
        let mut last_match = false;
        for c in candidate.to_lowercase().chars() {
            if qi < query_chars.len() && c == query_chars[qi] {
                score += if last_match { 10 } else { 1 };
                last_match = true;
                qi += 1;
            } else {
                last_match = false;
            }
        }
        if qi == query_chars.len() {
            scored.push((score, candidate));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().take(3).map(|(_, c)| c.clone()).collect()
}

/// Computes model suggestions for a `ModelNotFoundError`, mirroring
/// `modelSuggestions` in `provider.ts`.
pub fn model_suggestions(
    provider: &Info,
    model_id: &str,
    enable_experimental_models: bool,
) -> Vec<String> {
    let available: Vec<String> = provider
        .models
        .iter()
        .filter(|(_, model)| {
            if model.status == ModelStatus::Deprecated {
                return false;
            }
            if model.status == ModelStatus::Alpha && !enable_experimental_models {
                return false;
            }
            true
        })
        .map(|(id, _)| id.clone())
        .collect();

    let fuzzy = fuzzy_match(model_id, &available);
    if !fuzzy.is_empty() {
        return fuzzy;
    }

    let query: Vec<String> = model_id
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| part.len() > 1)
        .map(str::to_owned)
        .collect();
    if query.is_empty() {
        return Vec::new();
    }

    let mut items: Vec<(usize, String)> = available
        .into_iter()
        .filter_map(|id| {
            let score = query
                .iter()
                .filter(|part| id.to_lowercase().contains(part.as_str()))
                .count();
            if score > 0 {
                Some((score, id))
            } else {
                None
            }
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    items.into_iter().take(3).map(|(_, id)| id).collect()
}

/// `ModelNotFoundError` from `provider.ts`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelNotFoundError {
    pub provider_id: String,
    pub model_id: String,
    pub suggestions: Option<Vec<String>>,
}

impl std::fmt::Display for ModelNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let suggestions = match &self.suggestions {
            Some(suggestions) if !suggestions.is_empty() => {
                format!(" Did you mean: {}?", suggestions.join(", "))
            }
            _ => String::new(),
        };
        write!(
            f,
            "Model not found: {}/{}.{}",
            self.provider_id, self.model_id, suggestions
        )
    }
}

impl std::error::Error for ModelNotFoundError {}

impl ModelNotFoundError {
    pub fn new(
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        suggestions: Option<Vec<String>>,
    ) -> Self {
        ModelNotFoundError {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            suggestions,
        }
    }
}

/// `InitError` from `provider.ts`.
#[derive(Debug, thiserror::Error)]
#[error("Failed to initialize provider: {provider_id}")]
pub struct InitError {
    pub provider_id: String,
    #[source]
    pub cause: Option<anyhow::Error>,
}

/// `NoProvidersError` from `provider.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("No providers are available")]
pub struct NoProvidersError;

/// `NoModelsError` from `provider.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("No models are available for provider: {provider_id}")]
pub struct NoModelsError {
    pub provider_id: String,
}

/// `DefaultModelError` from `provider.ts`.
#[derive(Debug, thiserror::Error)]
pub enum DefaultModelError {
    #[error(transparent)]
    ModelNotFound(#[from] ModelNotFoundError),
    #[error(transparent)]
    NoProviders(#[from] NoProvidersError),
    #[error(transparent)]
    NoModels(#[from] NoModelsError),
}

const DEFAULT_NPM: &str = "@ai-sdk/openai-compatible";

/// Converts a models.dev `cost` into a registry `Cost`.
///
/// From `cost()` in `provider.ts`.
pub fn cost(c: &models_dev::Cost) -> Cost {
    let mut result = Cost {
        input: c.input.unwrap_or(0.0),
        output: c.output.unwrap_or(0.0),
        cache: CacheCost {
            read: c.cache_read.unwrap_or(0.0),
            write: c.cache_write.unwrap_or(0.0),
        },
        tiers: None,
        experimental_over_200k: None,
    };
    if let Some(tiers) = &c.tiers {
        result.tiers = Some(
            tiers
                .iter()
                .map(|item| CostTier {
                    input: item.input,
                    output: item.output,
                    cache: CacheCost {
                        read: item.cache_read.unwrap_or(0.0),
                        write: item.cache_write.unwrap_or(0.0),
                    },
                    tier: CostTierSize {
                        r#type: item.tier.r#type.clone(),
                        size: item.tier.size,
                    },
                })
                .collect(),
        );
    }
    if let Some(over) = &c.context_over_200k {
        result.experimental_over_200k = Some(ExperimentalOver200K {
            input: over.input.unwrap_or(0.0),
            output: over.output.unwrap_or(0.0),
            cache: CacheCost {
                read: over.cache_read.unwrap_or(0.0),
                write: over.cache_write.unwrap_or(0.0),
            },
        });
    }
    result
}

/// Converts a models.dev `Model` into a registry `Model`.
///
/// From `fromModelsDevModel()` in `provider.ts`.
pub fn from_models_dev_model(provider: &models_dev::Provider, model: &models_dev::Model) -> Model {
    let api_id = model.id.clone();
    let api_url = model
        .provider
        .as_ref()
        .and_then(|p| p.api.clone())
        .or_else(|| provider.api.clone())
        .unwrap_or_default();
    let api_npm = model
        .provider
        .as_ref()
        .and_then(|p| p.npm.clone())
        .or_else(|| provider.npm.clone())
        .unwrap_or_else(|| DEFAULT_NPM.to_string());

    let base = Model {
        id: model.id.clone(),
        provider_id: provider.id.clone(),
        name: model.name.clone(),
        family: model.family.clone(),
        api: ApiInfo {
            id: api_id,
            url: api_url,
            npm: api_npm,
        },
        status: match model.status {
            Some(CatalogModelStatus::Alpha) => ModelStatus::Alpha,
            Some(CatalogModelStatus::Beta) => ModelStatus::Beta,
            Some(CatalogModelStatus::Deprecated) => ModelStatus::Deprecated,
            None => ModelStatus::Active,
        },
        headers: std::collections::BTreeMap::new(),
        options: serde_json::Map::new(),
        cost: cost(&model.cost.clone().unwrap_or_default()),
        limit: Limit {
            context: model.limit.as_ref().and_then(|l| l.context).unwrap_or(0.0),
            input: model.limit.as_ref().and_then(|l| l.input),
            output: model.limit.as_ref().and_then(|l| l.output).unwrap_or(0.0),
        },
        capabilities: Capabilities {
            temperature: model.temperature.unwrap_or(false),
            reasoning: model.reasoning.unwrap_or(false),
            attachment: model.attachment.unwrap_or(false),
            toolcall: model.tool_call.unwrap_or(true),
            input: modalities(&model.modalities, "input"),
            output: modalities(&model.modalities, "output"),
            interleaved: match &model.interleaved {
                Some(models_dev::Interleaved::Bool(b)) => InterleavedField::Bool(*b),
                Some(models_dev::Interleaved::Field(f)) => {
                    InterleavedField::Field { field: f.clone() }
                }
                Some(models_dev::Interleaved::Struct { field }) => InterleavedField::Field {
                    field: field.clone(),
                },
                None => InterleavedField::Bool(false),
            },
        },
        release_date: model.release_date.clone().unwrap_or_default(),
        variants: IndexMap::new(),
    };

    let variants = match transform::reasoning_variants(model, &base) {
        Some(variants) => variants,
        None => transform::variants(&base),
    };
    let mut result = base;
    result.variants = variants;
    result
}

fn modalities(modalities: &Option<models_dev::Modalities>, which: &str) -> Modalities {
    let list = match (modalities, which) {
        (Some(m), "input") => &m.input,
        (Some(m), _) => &m.output,
        (None, _) => return Modalities::default(),
    };
    let contains = |modality: models_dev::Modality| list.contains(&modality);
    Modalities {
        text: contains(models_dev::Modality::Text),
        audio: contains(models_dev::Modality::Audio),
        image: contains(models_dev::Modality::Image),
        video: contains(models_dev::Modality::Video),
        pdf: contains(models_dev::Modality::Pdf),
    }
}

/// Converts a models.dev `Provider` into a registry `Info`.
///
/// From `fromModelsDevProvider()` in `provider.ts`.
pub fn from_models_dev_provider(provider: &models_dev::Provider) -> Info {
    let mut models: IndexMap<String, Model> = IndexMap::new();
    for (key, model) in &provider.models {
        models.insert(key.clone(), from_models_dev_model(provider, model));
        if let Some(experimental) = &model.experimental {
            if let Some(modes) = &experimental.modes {
                for (mode, opts) in modes {
                    let id = format!("{}-{}", model.id, mode);
                    let base = from_models_dev_model(provider, model);
                    let mut variant = base.clone();
                    variant.id = id.clone();
                    variant.name = {
                        let mut chars = mode.chars();
                        let capitalized = match chars.next() {
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                            None => String::new(),
                        };
                        format!("{} {}", model.name, capitalized)
                    };
                    if let Some(opts_cost) = &opts.cost {
                        let base_cost = serde_json::to_value(&variant.cost).unwrap_or(Value::Null);
                        let merged = merge_deep(
                            base_cost,
                            serde_json::to_value(cost(opts_cost)).unwrap_or(Value::Null),
                        );
                        variant.cost =
                            serde_json::from_value(merged).unwrap_or_else(|_| variant.cost.clone());
                    }
                    variant.options =
                        mode_options(&base, opts.provider.as_ref().and_then(|p| p.body.as_ref()));
                    if let Some(headers) = opts.provider.as_ref().and_then(|p| p.headers.as_ref()) {
                        variant.headers = headers.clone();
                    }
                    models.insert(id, variant);
                }
            }
        }
    }
    Info {
        id: provider.id.clone(),
        source: Source::Custom,
        name: provider.name.clone(),
        env: provider.env.clone(),
        options: serde_json::Map::new(),
        key: None,
        models,
    }
}

/// Computes the `options` for an experimental-mode model.
///
/// From `modeOptions()` in `provider.ts`.
pub fn mode_options(
    model: &Model,
    body: Option<&BTreeMap<String, Value>>,
) -> serde_json::Map<String, Value> {
    let Some(body) = body else {
        return model.options.clone();
    };
    let mut options = serde_json::Map::new();
    for (key, value) in body {
        let camel = camel_case_key(key);
        options.insert(camel, value.clone());
    }
    let reasoning = body.get("reasoning");
    let is_openai_npm = model.api.npm == "@ai-sdk/openai";
    let is_reasoning_record = matches!(reasoning, Some(Value::Object(_)));
    let reason_mode_is_string =
        matches!(reasoning, Some(Value::Object(o)) if o.get("mode").is_some_and(|m| m.is_string()));
    if !is_openai_npm || !is_reasoning_record || !reason_mode_is_string {
        return options;
    }
    options.remove("reasoning");
    let mode = reasoning.and_then(|r| r.get("mode")).cloned();
    if let Some(Value::String(mode)) = mode {
        options.insert("reasoningMode".to_string(), Value::String(mode));
    }
    options
}

fn camel_case_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut capitalize = false;
    for c in key.chars() {
        if c == '_' {
            capitalize = true;
        } else if capitalize {
            out.extend(c.to_uppercase());
            capitalize = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Strips runtime-only members from a provider before it crosses a public
/// boundary.
///
/// From `toPublicInfo()` in `provider.ts`. Rust values have no functions or
/// symbols, and all `Model` values are typed (always schema-valid), so this is
/// a clone.
pub fn to_public_info(provider: &Info) -> Info {
    let mut public = provider.clone();
    // `key` is an internal registry field used by the transport layer. The
    // catalog endpoints expose this value through `Info`, so never serialize
    // persisted or environment-provided credentials across that boundary.
    public.key = None;
    public
}

/// Computes the default model ID for every provider.
///
/// From `defaultModelIDs()` in `provider.ts`.
pub fn default_model_ids<T>(providers: &IndexMap<String, T>) -> IndexMap<String, String>
where
    T: ModelsIndex,
{
    let mut result = IndexMap::new();
    for (id, provider) in providers {
        let models = provider.model_values();
        if let Some(first) = sort(models).into_iter().next() {
            result.insert(id.clone(), first.id);
        }
    }
    result
}

pub trait ModelsIndex {
    fn model_values(&self) -> Vec<Model>;
}

/// Mirrors `Provider.defaultModel()` selection from `provider.ts`.
///
/// Selections, in priority order:
/// 1. An explicit `cfg.model` string (`provider/model`).
/// 2. The most recent `model.json` entry whose provider+model still exist in
///    the active registry (most-recent-first iteration).
/// 3. The first provider in registry order that is either the only candidate
///    when no config providers are declared, or that is config-declared. Its
///    first model after `sort()` is returned.
///
/// `recent` is ordered most-recent first to mirror the reference `model.json`
/// array ordering.
pub fn default_model(
    providers: &IndexMap<String, Info>,
    cfg_model: Option<&str>,
    configured_provider_ids: &[String],
    recent: &[(String, String)],
) -> Result<(String, String), DefaultModelError> {
    if let Some(model) = cfg_model {
        let (provider_id, model_id) = parse_model(model);
        if !provider_id.is_empty() && !model_id.is_empty() {
            return Ok((provider_id, model_id));
        }
    }

    for (provider_id, model_id) in recent {
        if let Some(provider) = providers.get(provider_id) {
            if provider.models.contains_key(model_id) {
                return Ok((provider_id.clone(), model_id.clone()));
            }
        }
    }

    let provider = providers
        .values()
        .find(|p| configured_provider_ids.is_empty() || configured_provider_ids.contains(&p.id))
        .ok_or(NoProvidersError)?;
    let model = sort(provider.models.values().cloned().collect::<Vec<_>>())
        .into_iter()
        .next()
        .ok_or_else(|| NoModelsError {
            provider_id: provider.id.clone(),
        })?;
    Ok((provider.id.clone(), model.id))
}

impl ModelsIndex for Info {
    fn model_values(&self) -> Vec<Model> {
        self.models.values().cloned().collect()
    }
}

const MODEL_PRIORITY: [&str; 4] = ["gpt-5", "claude-sonnet-4", "big-pickle", "gemini-3-pro"];

/// Sorts models by preferred-family priority, `latest` first, then id.
///
/// From `sort()` in `provider.ts`.
pub fn sort<T: HasId>(models: Vec<T>) -> Vec<T> {
    let mut items: Vec<(usize, u8, String, usize, T)> = models
        .into_iter()
        .enumerate()
        .map(|(index, model)| {
            let id = model.id().to_string();
            let priority = MODEL_PRIORITY
                .iter()
                .position(|filter| id.contains(filter))
                .map(|p| MODEL_PRIORITY.len() - p)
                .unwrap_or(0);
            let latest = if id.contains("latest") { 0 } else { 1 };
            (priority, latest, id, index, model)
        })
        .collect();
    items.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.3.cmp(&b.3))
    });
    items.into_iter().map(|(_, _, _, _, model)| model).collect()
}

pub trait HasId {
    fn id(&self) -> &str;
}

impl HasId for Model {
    fn id(&self) -> &str {
        &self.id
    }
}

/// Parses `"provider/model"` into its parts.
///
/// From `parseModel()` in `provider.ts`.
pub fn parse_model(model: &str) -> (String, String) {
    let mut parts = model.splitn(2, '/');
    let provider_id = parts.next().unwrap_or_default();
    let model_id = parts.next().unwrap_or_default();
    (provider_id.to_string(), model_id.to_string())
}

/// Deep-merges `b` into `a`, mirroring remeda's `mergeDeep`: objects merge
/// recursively, arrays and scalars are replaced by `b`.
pub fn merge_deep(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Object(mut a), Value::Object(b)) => {
            for (key, value) in b {
                match a.remove(&key) {
                    Some(existing) => {
                        a.insert(key, merge_deep(existing, value));
                    }
                    None => {
                        a.insert(key, value);
                    }
                }
            }
            Value::Object(a)
        }
        (_, b) => b,
    }
}

/// Merges a partial provider patch into `providers[provider_id]`, using the
/// catalog database entry as the base when no provider exists yet.
///
/// From `mergeProvider()` in `provider.ts`. `patch` is a partial object (any
/// subset of the `Info` fields); absent keys leave the base unchanged.
pub(crate) fn merge_provider(
    providers: &mut IndexMap<String, Info>,
    database: &IndexMap<String, Info>,
    provider_id: &str,
    patch: &serde_json::Map<String, Value>,
) {
    let existing = providers.get_mut(provider_id);
    let base = match existing {
        Some(_) => None,
        None => database.get(provider_id),
    };
    let base = match (existing, base) {
        (Some(existing), _) => {
            Some(serde_json::to_value(&*existing).unwrap_or(Value::Object(serde_json::Map::new())))
        }
        (None, Some(base)) => {
            Some(serde_json::to_value(base).unwrap_or(Value::Object(serde_json::Map::new())))
        }
        (None, None) => return,
    };
    let merged = merge_deep(base.unwrap(), Value::Object(patch.clone()));
    let info: Info = serde_json::from_value(merged).unwrap_or_else(|_| {
        // The patch always merges over a complete base, so this only runs if a
        // patch overwrote a required field with an incompatible value.
        database.get(provider_id).cloned().unwrap_or_else(|| {
            providers.get(provider_id).cloned().unwrap_or_else(|| Info {
                id: provider_id.to_string(),
                name: provider_id.to_string(),
                source: Source::Config,
                env: Vec::new(),
                key: None,
                options: serde_json::Map::new(),
                models: IndexMap::new(),
            })
        })
    });
    providers.insert(provider_id.to_string(), info);
}

pub use registry::{
    build_registry, build_registry_with_model_hooks, build_registry_with_npm_metadata, NpmMetadata,
    ProviderModelHookRegistration, RegistryInput,
};
