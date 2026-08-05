//! Generation / provider / http options, model selection and cache policy.
//! From reference/packages/llm/src/schema/options.ts

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use super::ids::{CacheHint, CacheHintType, JsonSchema, ModelId, ProviderId};
use super::messages::{Message, ToolDefinition};

/// `ProviderOptions` — `Record<string, Record<string, unknown>>`.
/// From reference/packages/llm/src/schema/options.ts (`ProviderOptions`)
pub type ProviderOptions = BTreeMap<String, BTreeMap<String, Value>>;

/// `mergeProviderOptions(...items)`.
/// From reference/packages/llm/src/schema/options.ts (`mergeProviderOptions`)
pub fn merge_provider_options(items: &[Option<&ProviderOptions>]) -> Option<ProviderOptions> {
    let mut result: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    for item in items.iter().flatten() {
        for (provider, options) in *item {
            let merged = merge_json_records(result.get(provider).map(|m| m.clone()), options);
            if let Some(merged) = merged {
                result.insert(provider.clone(), merged);
            }
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// `mergeJsonRecords(...)`.
/// From reference/packages/llm/src/schema/options.ts (`mergeJsonRecords`)
pub fn merge_json_records(
    base: Option<BTreeMap<String, Value>>,
    next: &BTreeMap<String, Value>,
) -> Option<BTreeMap<String, Value>> {
    let Some(mut result) = base else { return Some(next.clone()) };
    if next.len() == 1 {
        let (key, value) = next.iter().next().unwrap();
        result.insert(key.clone(), value.clone());
        return Some(result);
    }
    for (key, value) in next {
        if value.is_null() {
            continue;
        }
        let existing = result.get(key).cloned();
        let merged = match (existing, value) {
            (Some(Value::Object(left)), Value::Object(right)) => {
                let left_map: BTreeMap<String, Value> =
                    left.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let right_map: BTreeMap<String, Value> =
                    right.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                merge_json_records(Some(left_map), &right_map).map(|m| Value::Object(m.into_iter().collect()))
            }
            (_, v) => Some(v.clone()),
        };
        if let Some(merged) = merged {
            result.insert(key.clone(), merged);
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn merge_string_records(base: Option<BTreeMap<String, String>>, next: &BTreeMap<String, String>) -> Option<BTreeMap<String, String>> {
    let mut result = base.unwrap_or_default();
    for (k, v) in next {
        if !v.is_empty() {
            result.insert(k.clone(), v.clone());
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// `HttpOptions` — `{ body?, headers?, query? }`.
/// From reference/packages/llm/src/schema/options.ts (`HttpOptions`)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HttpOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<BTreeMap<String, String>>,
}

/// `mergeHttpOptions(...items)`.
/// From reference/packages/llm/src/schema/options.ts (`mergeHttpOptions`)
pub fn merge_http_options(items: &[Option<HttpOptions>]) -> Option<HttpOptions> {
    let body = merge_value_records(items.iter().map(|item| item.as_ref().and_then(|i| i.body.clone())));
    let headers = items.iter().fold(None, |acc: Option<BTreeMap<String, String>>, item| {
        let next = item.as_ref().and_then(|i| i.headers.clone());
        match (acc, next) {
            (Some(a), Some(b)) => merge_string_records(Some(a), &b),
            (a, b) => a.or(b),
        }
    });
    let query = items.iter().fold(None, |acc: Option<BTreeMap<String, String>>, item| {
        let next = item.as_ref().and_then(|i| i.query.clone());
        match (acc, next) {
            (Some(a), Some(b)) => merge_string_records(Some(a), &b),
            (a, b) => a.or(b),
        }
    });
    if body.is_none() && headers.is_none() && query.is_none() {
        None
    } else {
        Some(HttpOptions { body, headers, query })
    }
}

fn merge_value_records(items: impl Iterator<Item = Option<Value>>) -> Option<Value> {
    let mut acc: Option<BTreeMap<String, Value>> = None;
    for item in items.flatten() {
        if let Some(obj) = item.as_object() {
            let map: BTreeMap<String, Value> = obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            acc = match acc {
                Some(base) => merge_json_records(Some(base), &map),
                None => Some(map),
            };
        }
    }
    acc.map(|map| Value::Object(map.into_iter().collect()))
}

/// `GenerationOptions` — `{ maxTokens?, temperature?, topP?, topK?, frequencyPenalty?, presencePenalty?, seed?, stop? }`.
/// From reference/packages/llm/src/schema/options.ts (`GenerationOptions`)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GenerationOptions {
    #[serde(rename = "maxTokens", skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(rename = "topP", skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(rename = "topK", skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i64>,
    #[serde(rename = "frequencyPenalty", skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(rename = "presencePenalty", skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

/// `mergeGenerationOptions(...items)` — last defined value wins per field.
/// From reference/packages/llm/src/schema/options.ts (`mergeGenerationOptions`)
pub fn merge_generation_options(items: &[Option<&GenerationOptions>]) -> Option<GenerationOptions> {
    let mut result = GenerationOptions::default();
    for item in items.iter().flatten() {
        if item.max_tokens.is_some() {
            result.max_tokens = item.max_tokens;
        }
        if item.temperature.is_some() {
            result.temperature = item.temperature;
        }
        if item.top_p.is_some() {
            result.top_p = item.top_p;
        }
        if item.top_k.is_some() {
            result.top_k = item.top_k;
        }
        if item.frequency_penalty.is_some() {
            result.frequency_penalty = item.frequency_penalty;
        }
        if item.presence_penalty.is_some() {
            result.presence_penalty = item.presence_penalty;
        }
        if item.seed.is_some() {
            result.seed = item.seed;
        }
        if item.stop.is_some() {
            result.stop = item.stop.clone();
        }
    }
    if result.max_tokens.is_none()
        && result.temperature.is_none()
        && result.top_p.is_none()
        && result.top_k.is_none()
        && result.frequency_penalty.is_none()
        && result.presence_penalty.is_none()
        && result.seed.is_none()
        && result.stop.is_none()
    {
        None
    } else {
        Some(result)
    }
}

/// `ModelLimits` — `{ context?, output? }`.
/// From reference/packages/llm/src/schema/options.ts (`ModelLimits`)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<i64>,
}

/// `ModelDefaults` — `{ limits?, generation?, providerOptions?, http? }`.
/// From reference/packages/llm/src/schema/options.ts (`ModelDefaults`)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<ModelLimits>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<GenerationOptions>,
    #[serde(rename = "providerOptions", skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpOptions>,
}

impl ModelDefaults {
    /// `ModelDefaults.make(input)`.
    pub fn make(input: ModelDefaultsInput) -> ModelDefaults {
        match input {
            ModelDefaultsInput::Defaults(defaults) => defaults,
            ModelDefaultsInput::Fields { limits, generation, provider_options, http } => ModelDefaults {
                limits,
                generation,
                provider_options,
                http,
            },
        }
    }
}

pub enum ModelDefaultsInput {
    Defaults(ModelDefaults),
    Fields {
        limits: Option<ModelLimits>,
        generation: Option<GenerationOptions>,
        provider_options: Option<ProviderOptions>,
        http: Option<HttpOptions>,
    },
}

/// `ModelToolSchemaCompatibility` — tool-schema projection dialects.
/// From reference/packages/llm/src/schema/options.ts (`ModelToolSchemaCompatibility`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelToolSchemaCompatibility {
    Gemini,
    Moonshot,
}

/// `ModelCompatibility` — `{ toolSchema? }`.
/// From reference/packages/llm/src/schema/options.ts (`ModelCompatibility`)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelCompatibility {
    #[serde(rename = "toolSchema", skip_serializing_if = "Option::is_none")]
    pub tool_schema: Option<ModelToolSchemaCompatibility>,
}

/// `Model` — the selected model bound to a runnable route.
/// From reference/packages/llm/src/schema/options.ts (`Model`)
#[derive(Debug, Clone)]
pub struct Model {
    pub id: ModelId,
    pub provider: ProviderId,
    pub route: Arc<crate::route::Route>,
    pub defaults: Option<ModelDefaults>,
    pub compatibility: Option<ModelCompatibility>,
}

/// Serializable snapshot of a selected `Model` (the route object itself cannot
/// be serialized). Used by `PreparedRequest.model`.
/// From reference/packages/llm/src/schema/events.ts (`PreparedRequest`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSerializable {
    pub id: String,
    pub provider: String,
    pub route: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<ModelDefaults>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<ModelCompatibility>,
}

impl ModelSerializable {
    /// Build a serializable snapshot from a selected `Model`.
    pub fn from_model(model: &Model) -> ModelSerializable {
        ModelSerializable {
            id: model.id.0.clone(),
            provider: model.provider.0.clone(),
            route: model.route.id.clone(),
            defaults: model.defaults.clone(),
            compatibility: model.compatibility.clone(),
        }
    }
}

impl Model {
    /// `Model.make(input)`.
    pub fn make(input: ModelInput) -> Model {
        Model {
            id: ModelId::new(input.id),
            provider: ProviderId::new(input.provider),
            route: input.route,
            defaults: input.defaults,
            compatibility: input.compatibility,
        }
    }

    /// `Model.input(model)`.
    pub fn input(model: &Model) -> ModelInput {
        ModelInput {
            id: model.id.0.clone(),
            provider: model.provider.0.clone(),
            route: model.route.clone(),
            defaults: model.defaults.clone(),
            compatibility: model.compatibility.clone(),
        }
    }

    /// `Model.update(model, patch)`.
    pub fn update(model: &Model, patch: ModelInput) -> Model {
        let mut input = Model::input(model);
        input.id = patch.id;
        input.provider = patch.provider;
        input.route = patch.route;
        if patch.defaults.is_some() {
            input.defaults = patch.defaults;
        }
        if patch.compatibility.is_some() {
            input.compatibility = patch.compatibility;
        }
        Model::make(input)
    }
}

pub struct ModelInput {
    pub id: String,
    pub provider: String,
    pub route: Arc<crate::route::Route>,
    pub defaults: Option<ModelDefaults>,
    pub compatibility: Option<ModelCompatibility>,
}

/// `CachePolicyObject` — granular auto-placement policy.
/// From reference/packages/llm/src/schema/options.ts (`CachePolicyObject`)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CachePolicyObject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<CachePolicyMessages>,
    #[serde(rename = "ttlSeconds", skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CachePolicyMessages {
    LatestUserMessage,
    LatestAssistant,
    Tail { tail: usize },
}

impl Default for CachePolicyMessages {
    fn default() -> Self {
        CachePolicyMessages::LatestUserMessage
    }
}

/// `CachePolicy` — `"auto" | "none" | CachePolicyObject`.
/// From reference/packages/llm/src/schema/options.ts (`CachePolicy`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CachePolicy {
    Auto,
    None,
    Object(CachePolicyObject),
}

impl CacheHint {
    /// `makeHint(ttlSeconds)` — ephemeral hint.
    pub fn ephemeral(ttl_seconds: Option<u64>) -> CacheHint {
        CacheHint { kind: CacheHintType::Ephemeral, ttl_seconds }
    }
}

// Keep a compile-time reference to the message/tool types used by the
// cache-policy pass in `crate::cache_policy`.
#[allow(unused_imports)]
use super::messages as _messages;

pub use super::messages::{LlmRequest, LlmRequestInput, LlmRequestPatch, SystemPart};

/// Re-export `Message` and `ToolDefinition` for cache policy convenience.
/// From reference/packages/llm/src/cache-policy.ts (`applyCachePolicy`)
pub type CachePolicyMessage = Message;
pub type CachePolicyTool = ToolDefinition;

/// `CacheHint` placement helpers used by `applyCachePolicy`.
pub fn mark_last_message_content(
    messages: &[Message],
    index: usize,
    hint: CacheHint,
) -> Vec<Message> {
    let Some(target) = messages.get(index) else {
        return messages.to_vec();
    };
    if target.content.is_empty() {
        return messages.to_vec();
    }
    let last_text_index = target
        .content
        .iter()
        .rposition(|part| part.kind() == "text");
    let mark_at = match last_text_index {
        Some(i) => i,
        None => target.content.len() - 1,
    };
    let existing = &target.content[mark_at];
    let existing_cache = match existing {
        super::messages::ContentPart::Text { cache, .. } => cache.clone(),
        super::messages::ContentPart::ToolResult { cache, .. } => cache.clone(),
        _ => None,
    };
    if existing_cache.is_some() {
        return messages.to_vec();
    }
    let mut next_content = target.content.clone();
    next_content[mark_at] = set_part_cache(next_content[mark_at].clone(), hint);
    let mut next = messages.to_vec();
    next[index] = super::messages::Message {
        content: next_content,
        ..target.clone()
    };
    next
}

fn set_part_cache(part: super::messages::ContentPart, hint: CacheHint) -> super::messages::ContentPart {
    match part {
        super::messages::ContentPart::Text { text, cache: _, metadata, provider_metadata } => {
            super::messages::ContentPart::Text { text, cache: Some(hint), metadata, provider_metadata }
        }
        super::messages::ContentPart::ToolResult { id, name, result, provider_executed, cache: _, metadata, provider_metadata } => {
            super::messages::ContentPart::ToolResult {
                id,
                name,
                result,
                provider_executed,
                cache: Some(hint),
                metadata,
                provider_metadata,
            }
        }
        other => other,
    }
}
