//! Request options.
//!
//! From `transform.ts`: `options`, `smallOptions`, `providerOptions`,
//! `sdkKey`, `OUTPUT_TOKEN_MAX`.

use serde_json::{json, Value};

use crate::provider::Model;

use super::sampling::is_kimi_family;
use super::{JsonMap, INCLUDE_ENCRYPTED_REASONING};

/// Default cap for `maxOutputTokens`.
///
/// From `OUTPUT_TOKEN_MAX` in `transform.ts`.
pub const OUTPUT_TOKEN_MAX: f64 = 32_000.0;

/// Maps an npm package to the key the AI SDK expects for `providerOptions`.
///
/// From `sdkKey()` in `transform.ts`.
pub fn sdk_key(npm: &str) -> Option<&'static str> {
    match npm {
        "@ai-sdk/github-copilot" => Some("copilot"),
        "@ai-sdk/azure" => Some("azure"),
        "@ai-sdk/openai" => Some("openai"),
        "@ai-sdk/amazon-bedrock/mantle" => Some("openai"),
        "@ai-sdk/amazon-bedrock" => Some("bedrock"),
        "@ai-sdk/anthropic" | "@ai-sdk/google-vertex/anthropic" => Some("anthropic"),
        "@ai-sdk/google-vertex" => Some("vertex"),
        "@ai-sdk/google" => Some("google"),
        "@ai-sdk/alibaba" => Some("alibaba"),
        "@ai-sdk/cerebras" => Some("cerebras"),
        "@ai-sdk/cohere" => Some("cohere"),
        "@ai-sdk/deepinfra" => Some("deepinfra"),
        "@ai-sdk/groq" => Some("groq"),
        "@ai-sdk/mistral" => Some("mistral"),
        "@ai-sdk/perplexity" => Some("perplexity"),
        "@ai-sdk/togetherai" => Some("togetherai"),
        "@ai-sdk/vercel" => Some("vercel"),
        "@ai-sdk/xai" => Some("xai"),
        "venice-ai-sdk-provider" => Some("venice"),
        "@ai-sdk/gateway" => Some("gateway"),
        "@openrouter/ai-sdk-provider" => Some("openrouter"),
        "ai-gateway-provider" => Some("openaiCompatible"),
        _ => None,
    }
}

fn include_encrypted_reasoning() -> Value {
    Value::Array(
        INCLUDE_ENCRYPTED_REASONING
            .iter()
            .map(|s| Value::from(*s))
            .collect(),
    )
}

fn gpt_version(api_id: &str) -> (u32, u32) {
    let Some(idx) = api_id.find("gpt-") else {
        return (0, 0);
    };
    let rest = &api_id[idx + 4..];
    let Some(dot) = rest.find('.') else {
        return (0, 0);
    };
    let major: u32 = rest[..dot].parse().unwrap_or(0);
    let minor: u32 = rest[dot + 1..]
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("")
        .parse()
        .unwrap_or(0);
    (major, minor)
}

/// Computes the model options for a request.
///
/// From `options()` in `transform.ts`.
pub fn options(model: &Model, session_id: &str, provider_options: Option<&JsonMap>) -> JsonMap {
    let mut result = JsonMap::new();

    if model.api.npm == "@ai-sdk/google-vertex/anthropic"
        || (!model.api.id.contains("claude") && model.api.npm == "@ai-sdk/anthropic")
    {
        result.insert("toolStreaming".to_string(), Value::from(false));
    }

    if model.provider_id == "openai"
        || model.api.npm == "@ai-sdk/openai"
        || model.api.npm == "@ai-sdk/github-copilot"
        || model.api.npm == "@ai-sdk/amazon-bedrock/mantle"
        || model.api.npm == "@ai-sdk/xai"
    {
        result.insert("store".to_string(), Value::from(false));
    }
    if model.api.npm == "@ai-sdk/azure" {
        result.insert("store".to_string(), Value::from(false));
    }

    if model.api.npm == "@openrouter/ai-sdk-provider"
        || model.api.npm == "@llmgateway/ai-sdk-provider"
    {
        result.insert("usage".to_string(), json!({ "include": true }));
        if model.api.id.contains("gemini-3") {
            result.insert("reasoning".to_string(), json!({ "effort": "high" }));
        }
    }

    if model.provider_id == "baseten"
        || (model.provider_id == "opencode"
            && ["kimi-k2-thinking", "glm-4.6"].contains(&model.api.id.as_str()))
    {
        result.insert(
            "chat_template_args".to_string(),
            json!({ "enable_thinking": true }),
        );
    }

    if ["zai", "zhipuai"]
        .iter()
        .any(|id| model.provider_id.contains(id))
        && model.api.npm == "@ai-sdk/openai-compatible"
    {
        result.insert(
            "thinking".to_string(),
            json!({ "type": "enabled", "clear_thinking": false }),
        );
    }

    if model.provider_id == "meta" && model.api.npm == "@ai-sdk/openai" {
        result.insert("reasoningSummary".to_string(), Value::from("auto"));
        result.insert("include".to_string(), include_encrypted_reasoning());
    }

    if (model.api.npm == "@ai-sdk/google" || model.api.npm == "@ai-sdk/google-vertex")
        && model.capabilities.reasoning
    {
        let mut thinking_config = JsonMap::new();
        thinking_config.insert("includeThoughts".to_string(), Value::from(true));
        if model.api.id.contains("gemini-3") {
            thinking_config.insert("thinkingLevel".to_string(), Value::from("high"));
        }
        result.insert("thinkingConfig".to_string(), Value::Object(thinking_config));
    }

    let model_id = model.api.id.to_lowercase();

    if model_id.contains("minimax-m3") && model.api.npm == "@ai-sdk/anthropic" {
        result.insert("thinking".to_string(), json!({ "type": "adaptive" }));
    }

    if ["@ai-sdk/anthropic", "@ai-sdk/google-vertex/anthropic"].contains(&model.api.npm.as_str())
        && is_kimi_family(model)
        && model.capabilities.reasoning
    {
        result.insert(
            "thinking".to_string(),
            json!({ "type": "adaptive", "display": "summarized" }),
        );
        result.insert("effort".to_string(), Value::from("high"));
    }

    if model.provider_id == "alibaba-cn"
        && model.capabilities.reasoning
        && model.api.npm == "@ai-sdk/openai-compatible"
        && !model_id.contains("kimi-k2-thinking")
    {
        result.insert("enable_thinking".to_string(), Value::from(true));
    }

    let set_cache_key = provider_options
        .and_then(|o| o.get("setCacheKey"))
        .map(|v| v != &Value::Bool(false))
        .unwrap_or(true);
    if set_cache_key {
        if model.api.npm == "@ai-sdk/deepinfra" || model.api.npm == "@ai-sdk/cerebras" {
            result.insert("prompt_cache_key".to_string(), Value::from(session_id));
        } else if model.api.npm == "@ai-sdk/openai"
            || model.api.npm == "@ai-sdk/azure"
            || model.api.npm == "@ai-sdk/xai"
            || model.api.npm == "@ai-sdk/mistral"
            || model.api.npm == "venice-ai-sdk-provider"
            || provider_options.is_some_and(|o| o.get("setCacheKey") == Some(&Value::Bool(true)))
        {
            result.insert("promptCacheKey".to_string(), Value::from(session_id));
        }
    }

    if model.api.npm == "@ai-sdk/gateway" {
        result.insert("gateway".to_string(), json!({ "caching": "auto" }));
    }

    let (gpt_major, gpt_minor) = gpt_version(&model.api.id);
    let is_gpt55_or_newer = gpt_major > 5 || (gpt_major == 5 && gpt_minor >= 5);
    if model.api.npm == "@ai-sdk/azure"
        && provider_options.is_some_and(|o| o.get("useCompletionUrls").is_some())
    {
        if !is_gpt55_or_newer {
            result.insert("reasoningEffort".to_string(), Value::from("medium"));
        }
        return result;
    }

    if model.api.id.contains("gpt-5") && !model.api.id.contains("gpt-5-chat") {
        if !model.api.id.contains("gpt-5-pro") {
            result.insert("reasoningEffort".to_string(), Value::from("medium"));
            if [
                "@ai-sdk/openai",
                "@ai-sdk/azure",
                "@ai-sdk/github-copilot",
                "@ai-sdk/amazon-bedrock/mantle",
            ]
            .contains(&model.api.npm.as_str())
            {
                result.insert("reasoningSummary".to_string(), Value::from("auto"));
            }
            if ["@ai-sdk/openai", "@ai-sdk/amazon-bedrock/mantle"].contains(&model.api.npm.as_str())
            {
                result.insert("include".to_string(), include_encrypted_reasoning());
            }
        }
        if model.api.id.contains("gpt-5.")
            && !model.api.id.contains("codex")
            && !model.api.id.contains("-chat")
            && model.provider_id != "azure"
        {
            result.insert("textVerbosity".to_string(), Value::from("low"));
        }
        if model.provider_id.starts_with("opencode") && set_cache_key {
            result.insert("promptCacheKey".to_string(), Value::from(session_id));
            result.insert("include".to_string(), include_encrypted_reasoning());
            result.insert("reasoningSummary".to_string(), Value::from("auto"));
        }
    }

    result
}

/// Computes the small-model options for a request.
///
/// From `smallOptions()` in `transform.ts`.
pub fn small_options(model: &Model) -> JsonMap {
    let small = model.variants.values().next().cloned().unwrap_or_default();
    if model.provider_id == "openai"
        || model.api.npm == "@ai-sdk/openai"
        || model.api.npm == "@ai-sdk/github-copilot"
        || model.api.npm == "@ai-sdk/xai"
    {
        let merged = crate::provider::merge_deep(json!({ "store": false }), Value::Object(small));
        return merged.as_object().unwrap().clone();
    }
    if (model.provider_id == "openrouter" || model.provider_id == "llmgateway")
        && small.is_empty()
        && model.api.id.contains("google")
    {
        return json!({ "reasoning": { "enabled": false } })
            .as_object()
            .unwrap()
            .clone();
    }
    if model.provider_id == "venice" {
        if !small.is_empty() {
            return small;
        }
        return json!({ "veniceParameters": { "disableThinking": true } })
            .as_object()
            .unwrap()
            .clone();
    }
    small
}

/// Maps a model ID prefix to the provider slug used in `providerOptions`.
const SLUG_OVERRIDES: [(&str, &str); 1] = [("amazon", "bedrock")];

/// Computes the `providerOptions` namespace for a request.
///
/// From `providerOptions()` in `transform.ts`.
pub fn provider_options(model: &Model, options: &JsonMap) -> JsonMap {
    let uses_openai_reasoning_gate = [
        "@ai-sdk/openai",
        "@ai-sdk/azure",
        "@ai-sdk/amazon-bedrock/mantle",
    ]
    .contains(&model.api.npm.as_str());
    let has_reasoning_input =
        options.contains_key("reasoningEffort") || options.contains_key("reasoningSummary");
    let normalized: JsonMap =
        if uses_openai_reasoning_gate && (model.capabilities.reasoning || has_reasoning_input) {
            let mut normalized = options.clone();
            normalized.insert("forceReasoning".to_string(), Value::from(true));
            normalized
        } else {
            options.clone()
        };

    if model.api.npm == "@ai-sdk/gateway" {
        let raw_slug = model.api.id.split_once('/').map(|(head, _)| head);
        let slug = raw_slug.map(|raw| {
            SLUG_OVERRIDES
                .iter()
                .find(|(from, _)| *from == raw)
                .map(|(_, to)| *to)
                .unwrap_or(raw)
        });
        let gateway = normalized.get("gateway").cloned();
        let rest: JsonMap = normalized
            .iter()
            .filter(|(key, _)| key.as_str() != "gateway")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();

        let mut result = JsonMap::new();
        if let Some(gateway) = gateway {
            result.insert("gateway".to_string(), gateway);
        }
        if !rest.is_empty() {
            if let Some(slug) = slug {
                result.insert(slug.to_string(), Value::Object(rest));
            } else if let Some(Value::Object(gateway)) = result.get("gateway").cloned() {
                let mut merged = gateway;
                for (key, value) in rest {
                    merged.insert(key, value);
                }
                result.insert("gateway".to_string(), Value::Object(merged));
            } else {
                result.insert("gateway".to_string(), Value::Object(rest));
            }
        }
        return result;
    }

    let uses_dot_split_options = [
        "@ai-sdk/openai-compatible",
        "@ai-sdk/openai",
        "@ai-sdk/anthropic",
    ]
    .contains(&model.api.npm.as_str());
    let key = sdk_key(&model.api.npm)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if uses_dot_split_options {
                model
                    .provider_id
                    .split('.')
                    .next()
                    .unwrap_or(&model.provider_id)
                    .to_string()
            } else {
                model.provider_id.clone()
            }
        });

    if model.api.npm == "@ai-sdk/azure" {
        let mut result = JsonMap::new();
        result.insert("openai".to_string(), Value::Object(normalized.clone()));
        result.insert("azure".to_string(), Value::Object(normalized));
        return result;
    }
    let mut result = JsonMap::new();
    result.insert(key, Value::Object(normalized));
    result
}
