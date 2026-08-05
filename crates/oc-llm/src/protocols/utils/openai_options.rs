//! OpenAI provider options read from `request.providerOptions.openai`.
//! From reference/packages/llm/src/protocols/utils/openai-options.ts

use serde_json::Value;
use std::collections::BTreeMap;

use crate::schema::LlmRequest;
use crate::shared::is_record;

pub const OPENAI_REASONING_EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];

pub const OPENAI_RESPONSE_INCLUDABLES: [&str; 8] = [
    "file_search_call.results",
    "web_search_call.results",
    "web_search_call.action.sources",
    "message.input_image.image_url",
    "computer_call_output.output.image_url",
    "code_interpreter_call.outputs",
    "reasoning.encrypted_content",
    "message.output_text.logprobs",
];

pub const OPENAI_SERVICE_TIERS: [&str; 4] = ["auto", "default", "flex", "priority"];

pub const TEXT_VERBOSITY: [&str; 3] = ["low", "medium", "high"];

fn options(request: &LlmRequest) -> Option<&BTreeMap<String, Value>> {
    request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("openai"))
}

/// `OpenAIOptions.store(request)`.
pub fn store(request: &LlmRequest) -> Option<bool> {
    let value = options(request)?.get("store")?;
    value.as_bool()
}

/// `OpenAIOptions.reasoningEffort(request)`.
pub fn reasoning_effort(request: &LlmRequest) -> Option<String> {
    let value = options(request)?.get("reasoningEffort")?;
    let effort = value.as_str()?;
    if is_any_reasoning_effort(effort) {
        Some(effort.to_string())
    } else {
        None
    }
}

fn is_any_reasoning_effort(effort: &str) -> bool {
    crate::schema::REASONING_EFFORTS.contains(&effort)
}

/// `OpenAIOptions.isReasoningEffort(effort)`.
pub fn is_reasoning_effort(effort: &str) -> bool {
    OPENAI_REASONING_EFFORTS.contains(&effort)
}

/// `OpenAIOptions.reasoningSummary(request)`.
pub fn reasoning_summary(request: &LlmRequest) -> Option<&'static str> {
    let value = options(request)?.get("reasoningSummary")?;
    if value.as_str() == Some("auto") {
        Some("auto")
    } else {
        None
    }
}

/// `OpenAIOptions.include(request)`.
pub fn include(request: &LlmRequest) -> Option<Vec<String>> {
    let value = options(request)?.get("include")?;
    let array = value.as_array()?;
    let filtered: Vec<String> = array
        .iter()
        .filter_map(Value::as_str)
        .filter(|entry| OPENAI_RESPONSE_INCLUDABLES.contains(entry))
        .map(|entry| entry.to_string())
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

/// `OpenAIOptions.promptCacheKey(request)`.
pub fn prompt_cache_key(request: &LlmRequest) -> Option<String> {
    let value = options(request)?.get("promptCacheKey")?;
    value.as_str().map(|s| s.to_string())
}

/// `OpenAIOptions.textVerbosity(request)`.
pub fn text_verbosity(request: &LlmRequest) -> Option<String> {
    let value = options(request)?.get("textVerbosity")?;
    let verbosity = value.as_str()?;
    if TEXT_VERBOSITY.contains(&verbosity) {
        Some(verbosity.to_string())
    } else {
        None
    }
}

/// `OpenAIOptions.serviceTier(request)`.
pub fn service_tier(request: &LlmRequest) -> Option<String> {
    let value = options(request)?.get("serviceTier")?;
    let tier = value.as_str()?;
    if OPENAI_SERVICE_TIERS.contains(&tier) {
        Some(tier.to_string())
    } else {
        None
    }
}

/// `OpenAIOptions.instructions(request)`.
pub fn instructions(request: &LlmRequest) -> Option<String> {
    let value = options(request)?.get("instructions")?;
    value.as_str().map(|s| s.to_string())
}

/// Provider-options projection used by `providers/openai-options.ts`.
pub fn project_openai_options(input: &Value) -> Option<serde_json::Map<String, Value>> {
    if !is_record(input) {
        return None;
    }
    let obj = input.as_object().unwrap();
    let mut openai = serde_json::Map::new();
    for key in ["store", "promptCacheKey", "reasoningEffort", "reasoningSummary", "include", "textVerbosity", "serviceTier"] {
        if let Some(value) = obj.get(key) {
            openai.insert(key.to_string(), value.clone());
        }
    }
    if openai.is_empty() {
        None
    } else {
        Some(serde_json::Map::from_iter([("openai".to_string(), Value::Object(openai))]))
    }
}
