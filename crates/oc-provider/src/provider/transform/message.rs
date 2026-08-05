//! Message rewriting for provider requests.
//!
//! From `transform.ts`: `message`, `normalizeMessages`, `applyCaching`,
//! `unsupportedParts`, `mapProviderOptions`.
//!
//! Operates on the AI SDK `ModelMessage` shape (`@ai-sdk/provider`). Message
//! content parts are represented as JSON objects so fields the transform does
//! not touch survive unchanged.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::models_dev;
use crate::provider::{InterleavedField, Model};

use super::sampling::sanitize_surrogates;
use super::{sdk_key, JsonMap};

/// A message in the AI SDK `ModelMessage` shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMessage {
    pub role: String,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none", rename = "providerOptions")]
    pub provider_options: Option<JsonMap>,
}

/// Message content: a plain string or a list of content parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<JsonMap>),
}

impl MessageContent {
    pub fn as_parts(&self) -> Option<&[JsonMap]> {
        match self {
            MessageContent::Parts(parts) => Some(parts),
            _ => None,
        }
    }
}

fn part_type(part: &JsonMap) -> Option<&str> {
    part.get("type").and_then(|v| v.as_str())
}

fn part_text(part: &JsonMap) -> Option<&str> {
    part.get("text").and_then(|v| v.as_str())
}

/// Applies `sanitizeSurrogates` to the `output.value` of a tool-result part.
fn sanitize_tool_result_output(part: &mut JsonMap) {
    let Some(output) = part.get_mut("output") else {
        return;
    };
    let Value::Object(output) = output else {
        return;
    };
    let output_type = output
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    match output_type.as_str() {
        "text" | "error-text" => {
            if let Some(Value::String(value)) = output.get_mut("value") {
                *value = sanitize_surrogates(value);
            }
        }
        "content" => {
            if let Some(Value::Array(items)) = output.get_mut("value") {
                for item in items {
                    if let Value::Object(item) = item {
                        if item.get("type") == Some(&Value::from("text")) {
                            if let Some(Value::String(text)) = item.get_mut("text") {
                                *text = sanitize_surrogates(text);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Filters empty text/reasoning content for Anthropic and Bedrock, which
/// reject empty content.
///
/// From the Anthropic/Bedrock blocks of `normalizeMessages()` in `transform.ts`.
fn filter_empty_content(msg: ModelMessage, provider_namespace: &str) -> Option<ModelMessage> {
    match &msg.content {
        MessageContent::Text(text) => {
            if text.is_empty() {
                None
            } else {
                Some(msg)
            }
        }
        MessageContent::Parts(parts) => {
            let filtered: Vec<JsonMap> = parts
                .iter()
                .filter(|part| match part_type(part) {
                    Some("text") => part_text(part).is_some_and(|text| !text.is_empty()),
                    Some("reasoning") => {
                        let text = part_text(part).unwrap_or_default();
                        let has_signature = part
                            .get("providerOptions")
                            .and_then(|v| v.as_object())
                            .and_then(|o| o.get(provider_namespace))
                            .and_then(|v| v.as_object())
                            .is_some_and(|o| o.get("signature").is_some());
                        let has_redacted = part
                            .get("providerOptions")
                            .and_then(|v| v.as_object())
                            .and_then(|o| o.get(provider_namespace))
                            .and_then(|v| v.as_object())
                            .is_some_and(|o| o.get("redactedData").is_some());
                        !text.trim().is_empty() || has_signature || has_redacted
                    }
                    _ => true,
                })
                .cloned()
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(ModelMessage {
                    role: msg.role,
                    content: MessageContent::Parts(filtered),
                    provider_options: msg.provider_options,
                })
            }
        }
    }
}

fn scrub_tool_call_ids(msg: &mut ModelMessage, scrub: &impl Fn(&str) -> String) {
    let MessageContent::Parts(parts) = &mut msg.content else {
        return;
    };
    for part in parts.iter_mut() {
        let is_tool_part = matches!(part_type(part), Some("tool-call") | Some("tool-result"));
        if !is_tool_part {
            continue;
        }
        if let Some(Value::String(tool_call_id)) = part.get_mut("toolCallId") {
            *tool_call_id = scrub(tool_call_id);
        }
    }
}

/// Normalizes messages for a model.
///
/// From `normalizeMessages()` in `transform.ts`.
fn normalize_messages(msgs: Vec<ModelMessage>, model: &Model) -> Vec<ModelMessage> {
    let mut msgs: Vec<ModelMessage> = msgs
        .into_iter()
        .map(|mut msg| {
            match msg.role.as_str() {
                "tool" => {
                    if let MessageContent::Parts(parts) = &mut msg.content {
                        for part in parts {
                            if part_type(part) == Some("tool-result") {
                                sanitize_tool_result_output(part);
                            }
                        }
                    }
                }
                "system" => {
                    if let MessageContent::Text(text) = &mut msg.content {
                        *text = sanitize_surrogates(text);
                    }
                }
                "user" | "assistant" => match &mut msg.content {
                    MessageContent::Text(text) => *text = sanitize_surrogates(text),
                    MessageContent::Parts(parts) => {
                        for part in parts {
                            let is_text = matches!(
                                part.get("type").and_then(|v| v.as_str()),
                                Some("text") | Some("reasoning")
                            );
                            if is_text {
                                if let Some(Value::String(text)) = part.get_mut("text") {
                                    *text = sanitize_surrogates(text);
                                }
                            }
                            if part.get("type") == Some(&Value::from("tool-result")) {
                                sanitize_tool_result_output(part);
                            }
                        }
                    }
                },
                _ => {}
            }
            msg
        })
        .collect();

    if model.api.npm == "@ai-sdk/anthropic" {
        msgs = msgs
            .into_iter()
            .filter_map(|msg| filter_empty_content(msg, "anthropic"))
            .collect();
    }
    if model.api.npm == "@ai-sdk/amazon-bedrock" {
        msgs = msgs
            .into_iter()
            .filter_map(|msg| filter_empty_content(msg, "bedrock"))
            .collect();
    }

    if model.api.id.contains("claude") {
        let scrub = |id: &str| -> String {
            id.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect()
        };
        for msg in &mut msgs {
            if msg.role == "assistant" || msg.role == "tool" {
                scrub_tool_call_ids(msg, &scrub);
            }
        }
    }

    let model_id = model.api.id.to_lowercase();
    let is_mistral_family = model.provider_id == "mistral"
        || ["mistral", "devstral", "codestral", "pixtral", "mixtral"]
            .iter()
            .any(|family| model_id.contains(family));
    if is_mistral_family {
        return normalize_mistral(msgs);
    }

    if model.api.id.to_lowercase().contains("deepseek") {
        for msg in &mut msgs {
            if msg.role != "assistant" {
                continue;
            }
            match &mut msg.content {
                MessageContent::Parts(parts) => {
                    if parts
                        .iter()
                        .any(|part| part_type(part) == Some("reasoning"))
                    {
                        continue;
                    }
                    parts.push(
                        json!({ "type": "reasoning", "text": "" })
                            .as_object()
                            .unwrap()
                            .clone(),
                    );
                }
                MessageContent::Text(text) => {
                    let mut new_parts = Vec::new();
                    if !text.is_empty() {
                        new_parts.push(
                            json!({ "type": "text", "text": text.clone() })
                                .as_object()
                                .unwrap()
                                .clone(),
                        );
                    }
                    new_parts.push(
                        json!({ "type": "reasoning", "text": "" })
                            .as_object()
                            .unwrap()
                            .clone(),
                    );
                    msg.content = MessageContent::Parts(new_parts);
                }
            }
        }
    }

    if let InterleavedField::Field { field } = &model.capabilities.interleaved {
        if model.api.npm != "@openrouter/ai-sdk-provider" {
            let field = field.clone();
            for msg in &mut msgs {
                if msg.role != "assistant" {
                    continue;
                }
                let MessageContent::Parts(parts) = &msg.content else {
                    continue;
                };
                let reasoning_text: String = parts
                    .iter()
                    .filter(|part| part_type(part) == Some("reasoning"))
                    .filter_map(part_text)
                    .collect();
                let filtered: Vec<JsonMap> = parts
                    .iter()
                    .filter(|part| part_type(part) != Some("reasoning"))
                    .cloned()
                    .collect();
                let mut provider_options = msg.provider_options.clone().unwrap_or_default();
                let openai_compatible = provider_options
                    .entry("openaiCompatible".to_string())
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .expect("openaiCompatible is an object");
                openai_compatible.insert(field.clone(), Value::String(reasoning_text));
                msg.content = MessageContent::Parts(filtered);
                msg.provider_options = Some(provider_options);
            }
        }
    }

    msgs
}

/// Mistral family normalization: scrubs tool call IDs and inserts an assistant
/// message before user messages that would directly follow tool messages.
///
/// From the Mistral block of `normalizeMessages()` in `transform.ts`.
fn normalize_mistral(msgs: Vec<ModelMessage>) -> Vec<ModelMessage> {
    let scrub = |id: &str| -> String {
        let alnum: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(9)
            .collect();
        let mut padded = alnum;
        while padded.len() < 9 {
            padded.push('0');
        }
        padded
    };
    let mut result = Vec::with_capacity(msgs.len());
    for i in 0..msgs.len() {
        let mut msg = msgs[i].clone();
        if msg.role == "assistant" || msg.role == "tool" {
            scrub_tool_call_ids(&mut msg, &scrub);
        }
        let is_tool = msg.role == "tool";
        let next_is_user = msgs
            .get(i + 1)
            .map(|next| next.role == "user")
            .unwrap_or(false);
        result.push(msg);
        if is_tool && next_is_user {
            result.push(ModelMessage {
                role: "assistant".to_string(),
                content: MessageContent::Parts(vec![json!({ "type": "text", "text": "Done." })
                    .as_object()
                    .unwrap()
                    .clone()]),
                provider_options: None,
            });
        }
    }
    result
}

/// Applies caching provider options to the first two system messages and the
/// last two non-system messages.
///
/// From `applyCaching()` in `transform.ts`.
fn apply_caching(msgs: &mut Vec<ModelMessage>, model: &Model) {
    let provider_options = json!({
        "anthropic": { "cacheControl": { "type": "ephemeral" } },
        "openrouter": { "cacheControl": { "type": "ephemeral" } },
        "bedrock": { "cachePoint": { "type": "default" } },
        "openaiCompatible": { "cache_control": { "type": "ephemeral" } },
        "copilot": { "copilot_cache_control": { "type": "ephemeral" } },
        "alibaba": { "cacheControl": { "type": "ephemeral" } },
    })
    .as_object()
    .unwrap()
    .clone();

    let system_indices: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, msg)| msg.role == "system")
        .map(|(i, _)| i)
        .take(2)
        .collect();
    let non_system_indices: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, msg)| msg.role != "system")
        .map(|(i, _)| i)
        .collect();
    let final_indices: Vec<usize> = non_system_indices.into_iter().rev().take(2).collect();

    let mut processed = std::collections::HashSet::new();
    for index in system_indices.into_iter().chain(final_indices) {
        if !processed.insert(index) {
            continue;
        }
        let use_message_level_options = model.provider_id == "anthropic"
            || model.provider_id.contains("bedrock")
            || model.api.npm == "@ai-sdk/amazon-bedrock";
        let should_use_content_options = !use_message_level_options
            && matches!(&msgs[index].content, MessageContent::Parts(parts) if !parts.is_empty());

        if should_use_content_options {
            let last_content = msgs[index]
                .content
                .as_parts()
                .and_then(|parts| parts.last())
                .cloned();
            if let Some(mut last_content) = last_content {
                let is_approval = matches!(
                    part_type(&last_content),
                    Some("tool-approval-request") | Some("tool-approval-response")
                );
                if !is_approval {
                    let existing = last_content
                        .get("providerOptions")
                        .and_then(|v| v.as_object())
                        .cloned()
                        .unwrap_or_default();
                    let merged = crate::provider::merge_deep(
                        Value::Object(existing),
                        Value::Object(provider_options.clone()),
                    );
                    last_content.insert("providerOptions".to_string(), merged);
                    if let MessageContent::Parts(parts) = &mut msgs[index].content {
                        if let Some(last) = parts.last_mut() {
                            *last = last_content;
                        }
                    }
                    continue;
                }
            }
        }

        let existing = msgs[index].provider_options.clone().unwrap_or_default();
        msgs[index].provider_options = Some(
            crate::provider::merge_deep(
                Value::Object(existing),
                Value::Object(provider_options.clone()),
            )
            .as_object()
            .unwrap()
            .clone(),
        );
    }
}

fn map_provider_options(
    msgs: Vec<ModelMessage>,
    transform: &impl Fn(Option<JsonMap>) -> Option<JsonMap>,
) -> Vec<ModelMessage> {
    msgs.into_iter()
        .map(|mut msg| {
            msg.provider_options = transform(msg.provider_options.take());
            if let MessageContent::Parts(parts) = &mut msg.content {
                for part in parts.iter_mut() {
                    let is_approval = matches!(
                        part_type(part),
                        Some("tool-approval-request") | Some("tool-approval-response")
                    );
                    if is_approval {
                        continue;
                    }
                    let existing = part
                        .get("providerOptions")
                        .and_then(|v| v.as_object())
                        .cloned();
                    match transform(existing) {
                        Some(provider_options) => {
                            part.insert(
                                "providerOptions".to_string(),
                                Value::Object(provider_options),
                            );
                        }
                        None => {
                            part.remove("providerOptions");
                        }
                    }
                }
            }
            msg
        })
        .collect()
}

/// Replaces unsupported attachment parts with error text.
///
/// From `unsupportedParts()` in `transform.ts`.
fn unsupported_parts(msgs: Vec<ModelMessage>, model: &Model) -> Vec<ModelMessage> {
    msgs.into_iter()
        .map(|mut msg| {
            if msg.role != "user" {
                return msg;
            }
            let MessageContent::Parts(parts) = &msg.content else {
                return msg;
            };
            let filtered: Vec<JsonMap> = parts
                .iter()
                .map(|part| {
                    let part_type = part_type(part);
                    if !matches!(part_type, Some("file") | Some("image")) {
                        return part.clone();
                    }

                    if part_type == Some("image") {
                        let image = part.get("image").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                        if image.starts_with("data:") {
                            let (_, base64) = match image.split_once(";base64,") {
                                Some((prefix, base64)) => (prefix, base64),
                                None => (image.as_str(), ""),
                            };
                            if base64.is_empty() {
                                return json!({
                                    "type": "text",
                                    "text": "ERROR: Image file is empty or corrupted. Please provide a valid image."
                                })
                                .as_object()
                                .unwrap()
                                .clone();
                            }
                        }
                    }

                    let mime = if part_type == Some("image") {
                        part.get("image")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.split(';').next())
                            .unwrap_or_default()
                            .replacen("data:", "", 1)
                    } else {
                        part.get("mediaType").and_then(|v| v.as_str()).unwrap_or_default().to_string()
                    };
                    let filename = if part_type == Some("file") {
                        part.get("filename").and_then(|v| v.as_str()).map(str::to_string)
                    } else {
                        None
                    };
                    let Some(modality) = super::sampling::mime_to_modality(&mime) else {
                        return part.clone();
                    };
                    if model.capabilities.input.get(models_dev::Modality::from_str(modality).expect("known modality")) {
                        return part.clone();
                    }
                    let name = match &filename {
                        Some(filename) => format!("\"{}\"", filename),
                        None => modality.to_string(),
                    };
                    json!({
                        "type": "text",
                        "text": format!("ERROR: Cannot read {} (this model does not support {} input). Inform the user.", name, modality),
                    })
                    .as_object()
                    .unwrap()
                    .clone()
                })
                .collect();
            msg.content = MessageContent::Parts(filtered);
            msg
        })
        .collect()
}

/// Rewrites messages for a model request.
///
/// From `message()` in `transform.ts`.
pub fn message(msgs: Vec<ModelMessage>, model: &Model, options: &JsonMap) -> Vec<ModelMessage> {
    let mut msgs = unsupported_parts(msgs, model);
    msgs = normalize_messages(msgs, model);

    let uses_anthropic_automatic_caching = options.contains_key("cacheControl")
        && (model.api.npm == "@ai-sdk/anthropic"
            || model.api.npm == "@ai-sdk/google-vertex/anthropic");
    let is_anthropic_family = model.provider_id == "anthropic"
        || model.provider_id == "google-vertex-anthropic"
        || model.api.id.contains("anthropic")
        || model.api.id.contains("claude")
        || model.id.contains("anthropic")
        || model.id.contains("claude")
        || model.api.npm == "@ai-sdk/anthropic"
        || model.api.npm == "@ai-sdk/alibaba";
    if is_anthropic_family
        && model.api.npm != "@ai-sdk/gateway"
        && !uses_anthropic_automatic_caching
    {
        apply_caching(&mut msgs, model);
    }

    let key = sdk_key(&model.api.npm);
    if let Some(key) = key {
        if key != model.provider_id {
            let provider_id = model.provider_id.clone();
            let key = key.to_string();
            msgs = map_provider_options(msgs, &move |opts| {
                let mut opts = opts?;
                if !opts.contains_key(&provider_id) {
                    return Some(opts);
                }
                let value = opts.remove(&provider_id).expect("key present");
                opts.insert(key.clone(), value);
                Some(opts)
            });
        }
    }

    if options.get("store") != Some(&Value::Bool(true))
        && key.is_some()
        && [
            "@ai-sdk/openai",
            "@ai-sdk/azure",
            "@ai-sdk/amazon-bedrock/mantle",
            "@ai-sdk/github-copilot",
        ]
        .contains(&model.api.npm.as_str())
    {
        let key = key.unwrap().to_string();
        msgs = map_provider_options(msgs, &move |opts| {
            let mut opts = opts?;
            let Some(metadata) = opts.get(&key).cloned() else {
                return Some(opts);
            };
            let Some(mut metadata) = metadata.as_object().cloned() else {
                return Some(opts);
            };
            if !metadata.contains_key("itemId") {
                return Some(opts);
            }
            metadata.remove("itemId");
            opts.insert(key.clone(), Value::Object(metadata));
            Some(opts)
        });
    }

    msgs
}
