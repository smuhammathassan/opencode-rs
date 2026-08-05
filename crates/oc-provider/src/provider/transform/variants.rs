//! Reasoning-effort variants.
//!
//! From `transform.ts`: `variants`, `reasoningVariants`, and the effort
//! helpers.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::models_dev;
use crate::provider::Model;

use super::sampling::is_kimi_family;
use super::{JsonMap, VariantMap, INCLUDE_ENCRYPTED_REASONING, OUTPUT_TOKEN_MAX};

const WIDELY_SUPPORTED_EFFORTS: [&str; 3] = ["low", "medium", "high"];
const OPENAI_EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];
const OPENAI_GPT5_1_EFFORTS: [&str; 4] = ["none", "low", "medium", "high"];
const OPENAI_GPT5_2_PLUS_EFFORTS: [&str; 5] = ["none", "low", "medium", "high", "xhigh"];
const OPENAI_GPT5_PRO_EFFORTS: [&str; 1] = ["high"];
const OPENAI_GPT5_PRO_2_PLUS_EFFORTS: [&str; 3] = ["medium", "high", "xhigh"];
const OPENAI_GPT5_CHAT_EFFORTS: [&str; 1] = ["medium"];
const OPENAI_GPT5_CODEX_XHIGH_EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];
const OPENAI_GPT5_CODEX_3_PLUS_EFFORTS: [&str; 5] = ["none", "low", "medium", "high", "xhigh"];

const OPENAI_NONE_EFFORT_RELEASE_DATE: &str = "2025-11-13";
const OPENAI_XHIGH_EFFORT_RELEASE_DATE: &str = "2025-12-04";

const MISTRAL_REASONING_IDS: [&str; 4] = [
    "mistral-small-2603",
    "mistral-small-latest",
    "mistral-medium-3.5",
    "mistral-medium-2604",
];

fn gpt5_family_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|/)gpt-5(?:[.-]|$)").unwrap())
}

fn gpt5_version_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|/)gpt-5[.-](\d+)(?:[.-]|$)").unwrap())
}

fn gpt5_pro_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|/)gpt-5[.-]?pro(?:[.-]|$)").unwrap())
}

fn gpt5_versioned_pro_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|/)gpt-5[.-]\d+[.-]pro(?:[.-]|$)").unwrap())
}

fn claude_version_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"claude-(?:[a-z]+-)?(\d+)(?:[.-](\d{1,2}))?(?:[.@-]|$)").unwrap())
}

fn sap_o_model_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bo[1-9]").unwrap())
}

fn gpt5_version(api_id: &str) -> Option<u32> {
    gpt5_version_re()
        .captures(api_id)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn versioned_gpt5_reasoning_efforts(api_id: &str) -> Option<&'static [&'static str]> {
    if gpt5_versioned_pro_re().is_match(api_id) {
        return Some(&OPENAI_GPT5_PRO_2_PLUS_EFFORTS);
    }
    let version = gpt5_version(api_id)?;
    if version == 1 {
        return Some(&OPENAI_GPT5_1_EFFORTS);
    }
    Some(&OPENAI_GPT5_2_PLUS_EFFORTS)
}

fn gpt5_codex_reasoning_efforts(api_id: &str) -> Option<&'static [&'static str]> {
    if !gpt5_family_re().is_match(api_id) || !api_id.contains("codex") {
        return None;
    }
    let version = gpt5_version(api_id);
    if let Some(version) = version {
        if version >= 3 {
            return Some(&OPENAI_GPT5_CODEX_3_PLUS_EFFORTS);
        }
    }
    if api_id.contains("codex-max") || matches!(version, Some(v) if v >= 2) {
        return Some(&OPENAI_GPT5_CODEX_XHIGH_EFFORTS);
    }
    Some(&WIDELY_SUPPORTED_EFFORTS)
}

fn gpt5_chat_reasoning_efforts(api_id: &str) -> Option<Vec<&'static str>> {
    if !gpt5_family_re().is_match(api_id) || !api_id.contains("-chat") {
        return None;
    }
    if gpt5_version(api_id).is_none() {
        return Some(Vec::new());
    }
    Some(OPENAI_GPT5_CHAT_EFFORTS.to_vec())
}

fn openai_reasoning_efforts(api_id: &str, release_date: &str) -> Vec<&'static str> {
    let id = api_id.to_lowercase();
    if id.contains("deep-research") {
        return vec!["medium"];
    }
    if let Some(chat_efforts) = gpt5_chat_reasoning_efforts(&id) {
        return chat_efforts;
    }
    if gpt5_pro_re().is_match(&id) {
        return OPENAI_GPT5_PRO_EFFORTS.to_vec();
    }
    if let Some(codex_efforts) = gpt5_codex_reasoning_efforts(&id) {
        return codex_efforts.to_vec();
    }
    if let Some(versioned) = versioned_gpt5_reasoning_efforts(&id) {
        return versioned.to_vec();
    }
    let mut efforts = WIDELY_SUPPORTED_EFFORTS.to_vec();
    if gpt5_family_re().is_match(&id) {
        efforts.insert(0, "minimal");
    }
    if release_date >= OPENAI_NONE_EFFORT_RELEASE_DATE {
        efforts.insert(0, "none");
    }
    if release_date >= OPENAI_XHIGH_EFFORT_RELEASE_DATE {
        efforts.push("xhigh");
    }
    efforts
}

fn openai_compatible_reasoning_efforts(id: &str) -> Vec<&'static str> {
    let api_id = id.to_lowercase();
    if let Some(chat_efforts) = gpt5_chat_reasoning_efforts(&api_id) {
        return chat_efforts;
    }
    if gpt5_pro_re().is_match(&api_id) {
        return OPENAI_GPT5_PRO_EFFORTS.to_vec();
    }
    gpt5_codex_reasoning_efforts(&api_id)
        .or_else(|| versioned_gpt5_reasoning_efforts(&api_id))
        .map(|efforts| efforts.to_vec())
        .unwrap_or_else(|| OPENAI_EFFORTS.to_vec())
}

fn anthropic_uses_modern_adaptive_thinking(api_id: &str) -> bool {
    if !api_id.to_lowercase().contains("claude-") {
        return false;
    }
    let Some(caps) = claude_version_re().captures(api_id) else {
        return true;
    };
    let major: u32 = caps
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    let minor: u32 = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    major > 4 || (major == 4 && minor >= 7)
}

fn anthropic_opus45(api_id: &str) -> bool {
    ["opus-4-5", "opus-4.5"].iter().any(|v| api_id.contains(v))
}

fn anthropic_adaptive_efforts(api_id: &str) -> Option<Vec<&'static str>> {
    if anthropic_uses_modern_adaptive_thinking(api_id) {
        return Some(vec!["low", "medium", "high", "xhigh", "max"]);
    }
    let legacy = [
        "opus-4-6",
        "opus-4.6",
        "4-6-opus",
        "4.6-opus",
        "sonnet-4-6",
        "sonnet-4.6",
        "4-6-sonnet",
        "4.6-sonnet",
    ];
    if legacy.iter().any(|v| api_id.contains(v)) {
        return Some(vec!["low", "medium", "high", "max"]);
    }
    None
}

fn anthropic_omits_thinking(api_id: &str) -> bool {
    anthropic_uses_modern_adaptive_thinking(api_id)
}

fn google_thinking_level_efforts(api_id: &str) -> Vec<&'static str> {
    let id = api_id.to_lowercase();
    if !id.contains("gemini-3") {
        return vec!["low", "high"];
    }
    if id.contains("flash-image") {
        return vec!["minimal", "high"];
    }
    if id.contains("pro-image") {
        return vec!["high"];
    }
    if id.contains("flash") {
        return vec!["minimal", "low", "medium", "high"];
    }
    vec!["low", "medium", "high"]
}

fn google_thinking_budget_max(api_id: &str) -> f64 {
    let id = api_id.to_lowercase();
    if id.contains("2.5") && id.contains("pro") && !id.contains("flash") {
        return 32_768.0;
    }
    24_576.0
}

fn google_thinking_variants(model: &Model) -> VariantMap {
    let id = model.api.id.to_lowercase();
    let mut result = VariantMap::new();
    if id.contains("2.5") {
        result.insert(
            "high".to_string(),
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": 16_000 } })
                .as_object()
                .unwrap()
                .clone(),
        );
        result.insert(
            "max".to_string(),
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": google_thinking_budget_max(&id) } })
                .as_object()
                .unwrap()
                .clone(),
        );
        return result;
    }
    for effort in google_thinking_level_efforts(&id) {
        result.insert(
            effort.to_string(),
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingLevel": effort } })
                .as_object()
                .unwrap()
                .clone(),
        );
    }
    result
}

fn wrap_in_sap_model_params(variants: VariantMap) -> VariantMap {
    variants
        .into_iter()
        .map(|(key, value)| {
            let mut wrapped = JsonMap::new();
            wrapped.insert("modelParams".to_string(), Value::Object(value));
            (key, wrapped)
        })
        .collect()
}

fn from_pairs(pairs: Vec<(&str, Value)>) -> VariantMap {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.as_object().unwrap().clone()))
        .collect()
}

fn effort_map(efforts: Vec<&'static str>, body: impl Fn(&str) -> JsonMap) -> VariantMap {
    efforts
        .into_iter()
        .map(|effort| (effort.to_string(), body(effort)))
        .collect()
}

fn reasoning_effort_map(efforts: Vec<&'static str>) -> VariantMap {
    effort_map(efforts, |effort| {
        json!({ "reasoningEffort": effort }).as_object().unwrap().clone()
    })
}

fn openai_reasoning_options(effort: &str) -> JsonMap {
    json!({
        "reasoningEffort": effort,
        "reasoningSummary": "auto",
        "include": INCLUDE_ENCRYPTED_REASONING,
    })
    .as_object()
    .unwrap()
    .clone()
}

fn adaptive_thinking_map(adaptive_thinking_omitted: bool, effort: &str) -> JsonMap {
    let mut thinking = JsonMap::new();
    thinking.insert("type".to_string(), Value::from("adaptive"));
    if adaptive_thinking_omitted {
        thinking.insert("display".to_string(), Value::from("summarized"));
    }
    json!({ "thinking": thinking, "effort": effort }).as_object().unwrap().clone()
}

fn anthropic_opus45_effort(model: &Model, effort: &str) -> JsonMap {
    json!({
        "thinking": { "type": "enabled", "budgetTokens": (16_000.0_f64).min((model.limit.output / 2.0 - 1.0).floor()) },
        "effort": effort,
    })
    .as_object()
    .unwrap()
    .clone()
}

/// Computes the reasoning-effort variants for a model.
///
/// From `variants()` in `transform.ts`.
pub fn variants(model: &Model) -> VariantMap {
    if !model.capabilities.reasoning {
        return VariantMap::new();
    }

    let id = model.id.to_lowercase();
    let glm52 = ["glm-5.2", "glm-5-2", "glm-5p2"].iter().any(|name| {
        id.contains(name) || model.api.id.to_lowercase().contains(name)
    });

    if model.api.id.to_lowercase().contains("minimax-m3")
        && ["@ai-sdk/anthropic", "@ai-sdk/openai-compatible"].contains(&model.api.npm.as_str())
    {
        if ["nvidia", "lilac"].contains(&model.provider_id.as_str()) {
            return from_pairs(vec![
                ("none", json!({ "chat_template_kwargs": { "thinking_mode": "disabled" } })),
                ("thinking", json!({ "chat_template_kwargs": { "thinking_mode": "enabled" } })),
            ]);
        }
        return from_pairs(vec![
            ("none", json!({ "thinking": { "type": "disabled" } })),
            ("thinking", json!({ "thinking": { "type": "adaptive" } })),
        ]);
    }

    let adaptive_thinking_omitted = anthropic_omits_thinking(&model.api.id);
    let adaptive_efforts = anthropic_adaptive_efforts(&model.api.id);

    if glm52 && model.api.npm == "@openrouter/ai-sdk-provider" {
        return from_pairs(vec![
            ("high", json!({ "reasoning": { "effort": "high" } })),
            ("xhigh", json!({ "reasoning": { "effort": "xhigh" } })),
        ]);
    }
    if glm52 && model.api.npm == "@ai-sdk/openai-compatible" {
        return from_pairs(vec![
            ("high", json!({ "reasoningEffort": "high" })),
            ("max", json!({ "reasoningEffort": "max" })),
        ]);
    }
    if glm52 && model.api.npm == "@ai-sdk/anthropic" {
        return from_pairs(vec![
            ("high", json!({ "effort": "high" })),
            ("max", json!({ "effort": "max" })),
        ]);
    }

    if is_kimi_family(model)
        && ["@ai-sdk/anthropic", "@ai-sdk/google-vertex/anthropic"].contains(&model.api.npm.as_str())
    {
        return effort_map(vec!["low", "medium", "high", "xhigh", "max"], |effort| {
            json!({ "thinking": { "type": "adaptive", "display": "summarized" }, "effort": effort })
                .as_object()
                .unwrap()
                .clone()
        });
    }

    if id.contains("deepseek-chat")
        || id.contains("deepseek-reasoner")
        || id.contains("deepseek-r1")
        || id.contains("deepseek-v3")
        || id.contains("minimax")
        || (id.contains("glm") && !glm52)
        || id.contains("kimi")
        || id.contains("k2p")
        || id.contains("qwen")
        || id.contains("big-pickle")
    {
        return VariantMap::new();
    }

    if id.contains("grok") && id.contains("grok-3-mini") {
        if model.api.npm == "@openrouter/ai-sdk-provider" {
            return from_pairs(vec![
                ("low", json!({ "reasoning": { "effort": "low" } })),
                ("high", json!({ "reasoning": { "effort": "high" } })),
            ]);
        }
        return from_pairs(vec![
            ("low", json!({ "reasoningEffort": "low" })),
            ("high", json!({ "reasoningEffort": "high" })),
        ]);
    }

    match model.api.npm.as_str() {
        "@openrouter/ai-sdk-provider" => {
            let efforts = if model.api.id.starts_with("openai/") || id.contains("gpt") {
                openai_compatible_reasoning_efforts(&model.api.id)
            } else {
                WIDELY_SUPPORTED_EFFORTS.to_vec()
            };
            effort_map(efforts, |effort| {
                json!({ "reasoning": { "effort": effort } }).as_object().unwrap().clone()
            })
        }
        "ai-gateway-provider" => {
            if model.api.id.starts_with("openai/") {
                reasoning_effort_map(openai_reasoning_efforts(&model.api.id, &model.release_date))
            } else {
                reasoning_effort_map(WIDELY_SUPPORTED_EFFORTS.to_vec())
            }
        }
        "@ai-sdk/gateway" => {
            if model.api.id.contains("anthropic") {
                if let Some(efforts) = &adaptive_efforts {
                    return effort_map(efforts.clone(), |effort| {
                        adaptive_thinking_map(adaptive_thinking_omitted, effort)
                    });
                }
                return from_pairs(vec![
                    ("high", json!({ "thinking": { "type": "enabled", "budgetTokens": 16_000 } })),
                    ("max", json!({ "thinking": { "type": "enabled", "budgetTokens": 31_999 } })),
                ]);
            }
            if model.api.id.contains("google") {
                if model.api.id.contains("2.5") {
                    return from_pairs(vec![
                        ("high", json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": 16_000 } })),
                        (
                            "max",
                            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": google_thinking_budget_max(&model.api.id.to_lowercase()) } }),
                        ),
                    ]);
                }
                return effort_map(vec!["low", "high"], |effort| {
                    json!({ "includeThoughts": true, "thinkingLevel": effort }).as_object().unwrap().clone()
                });
            }
            reasoning_effort_map(openai_compatible_reasoning_efforts(&model.api.id))
        }
        "@ai-sdk/github-copilot" => {
            if model.id.contains("gemini") {
                return VariantMap::new();
            }
            if model.id.contains("claude") {
                return reasoning_effort_map(WIDELY_SUPPORTED_EFFORTS.to_vec());
            }
            let efforts: Vec<&'static str> =
                if id.contains("5.1-codex-max") || id.contains("5.2") || id.contains("5.3") {
                    OPENAI_GPT5_CODEX_XHIGH_EFFORTS.to_vec()
                } else {
                    let mut arr = WIDELY_SUPPORTED_EFFORTS.to_vec();
                    if id.contains("gpt-5") && model.release_date.as_str() >= "2025-12-04" {
                        arr.push("xhigh");
                    }
                    arr
                };
            effort_map(efforts, |effort| {
                json!({
                    "reasoningEffort": effort,
                    "reasoningSummary": "auto",
                    "include": INCLUDE_ENCRYPTED_REASONING,
                })
                .as_object()
                .unwrap()
                .clone()
            })
        }
        "@ai-sdk/cerebras"
        | "@ai-sdk/togetherai"
        | "@ai-sdk/xai"
        | "@ai-sdk/deepinfra"
        | "venice-ai-sdk-provider"
        | "@ai-sdk/openai-compatible" => {
            if model.api.id.to_lowercase().contains("north-mini-code") {
                return reasoning_effort_map(vec!["none", "high"]);
            }
            let mut efforts = WIDELY_SUPPORTED_EFFORTS.to_vec();
            if model.api.id.to_lowercase().contains("deepseek-v4") {
                efforts.push("max");
            }
            reasoning_effort_map(efforts)
        }
        "@ai-sdk/azure" => {
            if id == "o1-mini" {
                return VariantMap::new();
            }
            let efforts = openai_reasoning_efforts(&id, &model.release_date);
            effort_map(efforts, openai_reasoning_options)
        }
        "@ai-sdk/amazon-bedrock/mantle" | "@ai-sdk/openai" => {
            if model.provider_id == "meta" {
                return effort_map(OPENAI_EFFORTS.to_vec(), openai_reasoning_options);
            }
            let efforts = openai_reasoning_efforts(&model.api.id, &model.release_date);
            effort_map(efforts, openai_reasoning_options)
        }
        "@ai-sdk/anthropic" | "@ai-sdk/google-vertex/anthropic" => {
            if let Some(mut efforts) = adaptive_efforts {
                if model.provider_id == "github-copilot" {
                    if model.api.id.contains("opus-4.7") {
                        efforts = vec!["medium"];
                    }
                    efforts.retain(|v| *v != "max" && *v != "xhigh");
                }
                return effort_map(efforts, |effort| adaptive_thinking_map(adaptive_thinking_omitted, effort));
            }
            if anthropic_opus45(&model.api.id) {
                return effort_map(WIDELY_SUPPORTED_EFFORTS.to_vec(), |effort| {
                    anthropic_opus45_effort(model, effort)
                });
            }
            from_pairs(vec![
                (
                    "high",
                    json!({ "thinking": { "type": "enabled", "budgetTokens": (16_000.0_f64).min((model.limit.output / 2.0 - 1.0).floor()) } }),
                ),
                (
                    "max",
                    json!({ "thinking": { "type": "enabled", "budgetTokens": (31_999.0_f64).min(model.limit.output - 1.0) } }),
                ),
            ])
        }
        "@ai-sdk/amazon-bedrock" => {
            if let Some(efforts) = &adaptive_efforts {
                return effort_map(efforts.clone(), |effort| {
                    let mut reasoning_config = JsonMap::new();
                    reasoning_config.insert("type".to_string(), Value::from("adaptive"));
                    reasoning_config.insert("maxReasoningEffort".to_string(), Value::from(effort));
                    if adaptive_thinking_omitted {
                        reasoning_config.insert("display".to_string(), Value::from("summarized"));
                    }
                    json!({ "reasoningConfig": reasoning_config }).as_object().unwrap().clone()
                });
            }
            if model.api.id.contains("anthropic") {
                return from_pairs(vec![
                    ("high", json!({ "reasoningConfig": { "type": "enabled", "budgetTokens": 16_000 } })),
                    ("max", json!({ "reasoningConfig": { "type": "enabled", "budgetTokens": 31_999 } })),
                ]);
            }
            effort_map(WIDELY_SUPPORTED_EFFORTS.to_vec(), |effort| {
                json!({ "reasoningConfig": { "type": "enabled", "maxReasoningEffort": effort } })
                    .as_object()
                    .unwrap()
                    .clone()
            })
        }
        "@ai-sdk/google-vertex" | "@ai-sdk/google" => google_thinking_variants(model),
        "@ai-sdk/mistral" => {
            if !model.capabilities.reasoning {
                return VariantMap::new();
            }
            let mistral_id = model.api.id.to_lowercase();
            if !MISTRAL_REASONING_IDS.iter().any(|id| mistral_id.contains(id)) {
                return VariantMap::new();
            }
            from_pairs(vec![("high", json!({ "reasoningEffort": "high" }))])
        }
        "@ai-sdk/cohere" | "@ai-sdk/perplexity" => VariantMap::new(),
        "@ai-sdk/groq" => {
            let mut efforts = vec!["none"];
            efforts.extend(WIDELY_SUPPORTED_EFFORTS);
            reasoning_effort_map(efforts)
        }
        "@jerome-benoit/sap-ai-provider-v2" => {
            if id.contains("anthropic") {
                if let Some(efforts) = &adaptive_efforts {
                    let result = effort_map(efforts.clone(), |effort| {
                        let mut thinking = JsonMap::new();
                        thinking.insert("type".to_string(), Value::from("adaptive"));
                        if adaptive_thinking_omitted {
                            thinking.insert("display".to_string(), Value::from("summarized"));
                        }
                        json!({ "thinking": thinking, "output_config": { "effort": effort } })
                            .as_object()
                            .unwrap()
                            .clone()
                    });
                    return wrap_in_sap_model_params(result);
                }
                return wrap_in_sap_model_params(from_pairs(vec![
                    ("high", json!({ "thinking": { "type": "enabled", "budget_tokens": 16_000 } })),
                    ("max", json!({ "thinking": { "type": "enabled", "budget_tokens": 31_999 } })),
                ]));
            }
            if id.contains("gemini") && id.contains("2.5") {
                return wrap_in_sap_model_params(google_thinking_variants(model));
            }
            if id.contains("gpt") || sap_o_model_re().is_match(&id) {
                let efforts = openai_reasoning_efforts(&id, &model.release_date);
                return wrap_in_sap_model_params(effort_map(efforts, |effort| {
                    json!({ "reasoning_effort": effort }).as_object().unwrap().clone()
                }));
            }
            wrap_in_sap_model_params(effort_map(vec!["low", "medium", "high"], |effort| {
                json!({ "reasoning_effort": effort }).as_object().unwrap().clone()
            }))
        }
        _ => VariantMap::new(),
    }
}

/// Computes reasoning variants from models.dev `reasoning_options`.
///
/// From `reasoningVariants()` in `transform.ts`. Returns `None` when the
/// catalog carries no `reasoning_options` metadata, mirroring the reference's
/// `undefined` return.
pub fn reasoning_variants(model: &models_dev::Model, target: &Model) -> Option<VariantMap> {
    let options = model.reasoning_options.as_ref()?;
    if options.is_empty() {
        return Some(VariantMap::new());
    }

    let effort = options
        .iter()
        .find(|o| matches!(o, models_dev::ReasoningOption::Effort { .. }));
    if let Some(models_dev::ReasoningOption::Effort { values }) = effort {
        return Some(effort_variants(target, values));
    }

    let toggle = options
        .iter()
        .any(|o| matches!(o, models_dev::ReasoningOption::Toggle));
    let budget = options
        .iter()
        .find(|o| matches!(o, models_dev::ReasoningOption::BudgetTokens { .. }));

    let Some(models_dev::ReasoningOption::BudgetTokens { min, max }) = budget else {
        if toggle {
            return non_empty_variants(reasoning_toggle(target));
        }
        return None;
    };

    let mut merged = VariantMap::new();
    if toggle {
        for (key, value) in reasoning_toggle(target) {
            merged.insert(key, value);
        }
    }
    for (key, value) in budget_variants(target, *min, *max) {
        merged.insert(key, value);
    }
    non_empty_variants(merged)
}

fn effort_variants(model: &Model, values: &[Option<String>]) -> VariantMap {
    let mut result = VariantMap::new();
    for value in values {
        let Some(id) = value else {
            if let Some(settings) = reasoning_effort(model, "none") {
                result.insert("none".to_string(), settings);
            }
            continue;
        };
        if let Some(settings) = reasoning_effort(model, id) {
            result.insert(id.clone(), settings);
        }
    }
    result
}

fn budget_variants(model: &Model, min: Option<f64>, max: Option<f64>) -> VariantMap {
    let maximum = (max.unwrap_or(OUTPUT_TOKEN_MAX - 1.0))
        .min(model.limit.output - 1.0)
        .min(OUTPUT_TOKEN_MAX - 1.0);
    if maximum <= 0.0 {
        return VariantMap::new();
    }
    let high = (min.unwrap_or(0.0))
        .max(((maximum + 1.0) / 2.0).floor())
        .min(maximum);
    let mut result = VariantMap::new();
    for (id, budget) in [("high", high), ("max", maximum)] {
        if let Some(settings) = reasoning_budget(model, budget) {
            result.insert(id.to_string(), settings);
        }
    }
    result
}

fn non_empty_variants(variants: VariantMap) -> Option<VariantMap> {
    if variants.is_empty() {
        None
    } else {
        Some(variants)
    }
}

fn reasoning_toggle(model: &Model) -> VariantMap {
    if model.api.npm == "@ai-sdk/alibaba" {
        return from_pairs(vec![
            ("none", json!({ "enableThinking": false })),
            ("high", json!({ "enableThinking": true })),
        ]);
    }
    if model.api.npm == "@ai-sdk/cohere" {
        return from_pairs(vec![
            ("none", json!({ "thinking": { "type": "disabled" } })),
            ("high", json!({ "thinking": { "type": "enabled" } })),
        ]);
    }
    VariantMap::new()
}

fn reasoning_effort(model: &Model, effort: &str) -> Option<JsonMap> {
    match model.api.npm.as_str() {
        "@openrouter/ai-sdk-provider" => {
            Some(json!({ "reasoning": { "effort": effort } }).as_object().unwrap().clone())
        }
        "@ai-sdk/anthropic" | "@ai-sdk/google-vertex/anthropic" => anthropic_effort(model, effort)
            .or_else(|| Some(json!({ "effort": effort }).as_object().unwrap().clone())),
        "@ai-sdk/google" | "@ai-sdk/google-vertex" => Some(
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingLevel": effort } })
                .as_object()
                .unwrap()
                .clone(),
        ),
        "@ai-sdk/amazon-bedrock" => {
            if anthropic_adaptive_efforts(&model.api.id).is_some() {
                let mut config = JsonMap::new();
                config.insert("type".to_string(), Value::from("adaptive"));
                config.insert("maxReasoningEffort".to_string(), Value::from(effort));
                if anthropic_omits_thinking(&model.api.id) {
                    config.insert("display".to_string(), Value::from("summarized"));
                }
                return Some(json!({ "reasoningConfig": config }).as_object().unwrap().clone());
            }
            if anthropic_opus45(&model.api.id) {
                return Some(
                    json!({
                        "reasoningConfig": {
                            "type": "enabled",
                            "budgetTokens": (16_000.0_f64).min((model.limit.output / 2.0 - 1.0).floor()),
                            "maxReasoningEffort": effort,
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                );
            }
            if model.api.id.contains("anthropic") {
                return None;
            }
            Some(
                json!({ "reasoningConfig": { "type": "enabled", "maxReasoningEffort": effort } })
                    .as_object()
                    .unwrap()
                    .clone(),
            )
        }
        "@ai-sdk/gateway" => {
            if model.id.contains("anthropic") {
                Some(
                    json!({ "thinking": { "type": "adaptive", "display": "summarized" }, "effort": effort })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            } else if model.id.contains("google") {
                Some(
                    json!({ "thinkingConfig": { "includeThoughts": true, "thinkingLevel": effort } })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            } else {
                Some(json!({ "reasoningEffort": effort }).as_object().unwrap().clone())
            }
        }
        "@ai-sdk/github-copilot" => {
            if model.id.contains("gemini") {
                None
            } else if model.id.contains("claude") {
                Some(json!({ "reasoningEffort": effort }).as_object().unwrap().clone())
            } else {
                Some(
                    json!({
                        "reasoningEffort": effort,
                        "reasoningSummary": "auto",
                        "include": INCLUDE_ENCRYPTED_REASONING,
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                )
            }
        }
        "@ai-sdk/openai" | "@ai-sdk/amazon-bedrock/mantle" | "@ai-sdk/azure" => {
            Some(openai_reasoning_options(effort))
        }
        "@jerome-benoit/sap-ai-provider-v2" => {
            if model.id.contains("anthropic") {
                Some(
                    json!({ "modelParams": { "thinking": { "type": "adaptive", "display": "summarized" }, "output_config": { "effort": effort } } })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            } else {
                Some(
                    json!({ "modelParams": { "reasoning_effort": effort } })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            }
        }
        "@ai-sdk/openai-compatible"
        | "@ai-sdk/xai"
        | "@ai-sdk/mistral"
        | "@ai-sdk/groq"
        | "@ai-sdk/cerebras"
        | "@ai-sdk/deepinfra"
        | "@ai-sdk/togetherai"
        | "venice-ai-sdk-provider"
        | "ai-gateway-provider" => Some(json!({ "reasoningEffort": effort }).as_object().unwrap().clone()),
        "@ai-sdk/cohere" | "@ai-sdk/perplexity" | "@ai-sdk/vercel" | "@ai-sdk/alibaba" | "gitlab-ai-provider" => {
            None
        }
        _ => None,
    }
}

fn anthropic_effort(model: &Model, effort: &str) -> Option<JsonMap> {
    if anthropic_opus45(&model.api.id) {
        return Some(anthropic_opus45_effort(model, effort));
    }
    if is_kimi_family(model) {
        return Some(
            json!({ "thinking": { "type": "adaptive", "display": "summarized" }, "effort": effort })
                .as_object()
                .unwrap()
                .clone(),
        );
    }
    if anthropic_adaptive_efforts(&model.api.id).is_none() {
        return None;
    }
    Some(adaptive_thinking_map(anthropic_omits_thinking(&model.api.id), effort))
}

fn reasoning_budget(model: &Model, budget: f64) -> Option<JsonMap> {
    match model.api.npm.as_str() {
        "@openrouter/ai-sdk-provider" => {
            Some(json!({ "reasoning": { "max_tokens": budget } }).as_object().unwrap().clone())
        }
        "@ai-sdk/anthropic" | "@ai-sdk/google-vertex/anthropic" => {
            Some(json!({ "thinking": { "type": "enabled", "budgetTokens": budget } }).as_object().unwrap().clone())
        }
        "@ai-sdk/google" | "@ai-sdk/google-vertex" => Some(
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": budget } })
                .as_object()
                .unwrap()
                .clone(),
        ),
        "@ai-sdk/amazon-bedrock" => {
            Some(json!({ "reasoningConfig": { "type": "enabled", "budgetTokens": budget } }).as_object().unwrap().clone())
        }
        "@ai-sdk/gateway" => {
            if model.id.contains("anthropic") {
                Some(
                    json!({ "thinking": { "type": "enabled", "budgetTokens": budget } })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            } else if model.id.contains("google") {
                Some(
                    json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": budget } })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            } else {
                None
            }
        }
        "@ai-sdk/cohere" => {
            Some(json!({ "thinking": { "type": "enabled", "tokenBudget": budget } }).as_object().unwrap().clone())
        }
        "@ai-sdk/alibaba" => {
            Some(json!({ "enableThinking": true, "thinkingBudget": budget }).as_object().unwrap().clone())
        }
        "@jerome-benoit/sap-ai-provider-v2" => {
            if model.id.contains("anthropic") {
                Some(
                    json!({ "modelParams": { "thinking": { "type": "enabled", "budget_tokens": budget } } })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            } else if model.id.contains("gemini") {
                Some(
                    json!({ "modelParams": { "thinkingConfig": { "includeThoughts": true, "thinkingBudget": budget } } })
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            } else {
                None
            }
        }
        _ => None,
    }
}
