//! Provider option normalization for OpenAI-compatible facades.
//! From reference/packages/llm/src/providers/openai-options.ts

use serde_json::Value;
use std::collections::BTreeMap;

use crate::schema::merge_provider_options;
use crate::schema::ProviderOptions;
use crate::shared::is_record;

/// `definedEntries` — drop `undefined` entries.
fn defined_entries(input: &serde_json::Map<String, Value>) -> BTreeMap<String, Value> {
    input
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// `openAIProviderOptions(options)`.
/// From reference/packages/llm/src/providers/openai-options.ts
fn openai_provider_options(options: &Value) -> Option<ProviderOptions> {
    let mut openai = BTreeMap::new();
    if is_record(options) {
        let obj = options.as_object().unwrap();
        for key in ["store", "promptCacheKey", "reasoningEffort", "reasoningSummary", "include", "textVerbosity", "serviceTier"] {
            if let Some(value) = obj.get(key) {
                openai.insert(key.to_string(), value.clone());
            }
        }
    }
    if openai.is_empty() {
        None
    } else {
        Some(ProviderOptions::from_iter([("openai".to_string(), openai)]))
    }
}

/// `gpt5DefaultOptions(modelID, options)`.
/// From reference/packages/llm/src/providers/openai-options.ts (`gpt5DefaultOptions`)
pub fn gpt5_default_options(model_id: &str, text_verbosity: bool) -> Option<ProviderOptions> {
    let id = model_id.to_lowercase();
    if !id.contains("gpt-5") || id.contains("gpt-5-chat") || id.contains("gpt-5-pro") {
        return None;
    }
    let mut options = serde_json::Map::new();
    options.insert("reasoningEffort".to_string(), Value::String("medium".to_string()));
    options.insert("reasoningSummary".to_string(), Value::String("auto".to_string()));
    options.insert(
        "include".to_string(),
        Value::Array(vec![Value::String("reasoning.encrypted_content".to_string())]),
    );
    if text_verbosity && id.contains("gpt-5.") && !id.contains("codex") && !id.contains("-chat") {
        options.insert("textVerbosity".to_string(), Value::String("low".to_string()));
    }
    openai_provider_options(&Value::Object(options))
}

/// `openAIDefaultOptions(modelID, options)`.
/// From reference/packages/llm/src/providers/openai-options.ts (`openAIDefaultOptions`)
pub fn openai_default_options(model_id: &str, text_verbosity: bool) -> Option<ProviderOptions> {
    let mut store = serde_json::Map::new();
    store.insert("store".to_string(), Value::Bool(false));
    merge_provider_options(&[
        openai_provider_options(&Value::Object(store)).as_ref(),
        gpt5_default_options(model_id, text_verbosity).as_ref(),
    ])
}

/// `withOpenAIOptions(modelID, options, defaults)`.
/// From reference/packages/llm/src/providers/openai-options.ts (`withOpenAIOptions`)
pub fn with_openai_options(
    model_id: &str,
    provider_options: Option<ProviderOptions>,
    text_verbosity: bool,
) -> Option<ProviderOptions> {
    merge_provider_options(&[openai_default_options(model_id, text_verbosity).as_ref(), provider_options.as_ref()])
}

/// Raw `OpenAIOptionsInput` projection used by provider facades.
pub fn project_openai_options(input: &Value) -> Option<ProviderOptions> {
    if !is_record(input) {
        return None;
    }
    let obj = input.as_object().unwrap();
    let entries = defined_entries(obj);
    if entries.is_empty() {
        return None;
    }
    Some(ProviderOptions::from_iter([("openai".to_string(), entries)]))
}
