//! Shared toolkit used by protocol implementations.
//! From reference/packages/llm/src/protocols/shared.ts

use serde_json::Value;

use crate::schema::messages::{
    ContentPart, MediaData, MediaPart, SystemPart, ToolFileContent, ToolResultPart, ToolResultValue,
};
use crate::schema::LlmError;

/// Insert a key into a JSON object map, preserving insertion order.
#[macro_export]
macro_rules! jset {
    ($obj:expr, $key:expr, $value:expr) => {{
        $obj.insert(String::from($key), serde_json::Value::from($value));
    }};
}

/// Insert an optional value, omitting the key when `None`.
#[macro_export]
macro_rules! jset_opt {
    ($obj:expr, $key:expr, $value:expr) => {{
        if let Some(v) = $value {
            $obj.insert(String::from($key), serde_json::Value::from(v));
        }
    }};
}

/// `isRecord` — plain-object narrowing (excludes arrays).
/// From reference/packages/llm/src/utils/record.ts
pub fn is_record(value: &Value) -> bool {
    value.is_object()
}

/// `ProviderShared.encodeJson` — serialize with `serde_json`.
pub fn encode_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// `ProviderShared.decodeJson` — parse a JSON string.
pub fn decode_json(input: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(input)
}

/// `ProviderShared.joinText(parts)`.
/// From reference/packages/llm/src/protocols/shared.ts (`joinText`)
pub fn join_text(parts: &[impl AsRef<str>]) -> String {
    parts
        .iter()
        .map(|part| part.as_ref())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `joinText` over text-part-like items.
pub fn join_text_parts<'a, T: Iterator<Item = &'a str>>(parts: T) -> String {
    parts.collect::<Vec<_>>().join("\n")
}

/// Escape text for the `<system-update>` wrapper.
/// From reference/packages/llm/src/protocols/shared.ts (`wrapSystemUpdate`)
fn escape_system_update_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `ProviderShared.wrapSystemUpdate(parts)`.
/// From reference/packages/llm/src/protocols/shared.ts (`wrapSystemUpdate`)
pub fn wrap_system_update(parts: &[String]) -> String {
    format!("<system-update>\n{}\n</system-update>", escape_system_update_text(&parts.join("\n")))
}

/// `ProviderShared.systemUpdateText(route, message)` — extract text parts only.
/// From reference/packages/llm/src/protocols/shared.ts (`systemUpdateText`)
pub fn system_update_text(route: &str, message: &crate::schema::Message) -> Result<Vec<crate::schema::TextPart>, LlmError> {
    let mut content = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text { text, .. } => content.push(crate::schema::TextPart::make(text)),
            other => {
                return Err(unsupported_content(
                    route,
                    &message.role.to_string(),
                    &["text"],
                    format!("only support {} content for now", other.kind()),
                ));
            }
        }
    }
    Ok(content)
}

/// `ProviderShared.wrappedSystemUpdate(route, message)`.
/// From reference/packages/llm/src/protocols/shared.ts (`wrappedSystemUpdate`)
pub fn wrapped_system_update(route: &str, message: &crate::schema::Message) -> Result<WrappedSystemUpdate, LlmError> {
    let content = system_update_text(route, message)?;
    let cache = content.last().and_then(|part| part.cache.clone());
    let text = wrap_system_update(&content.iter().map(|part| part.text.clone()).collect::<Vec<_>>());
    Ok(WrappedSystemUpdate { text, cache })
}

pub struct WrappedSystemUpdate {
    pub text: String,
    pub cache: Option<crate::schema::CacheHint>,
}

/// `ProviderShared.parseToolInput(route, name, raw)`.
/// From reference/packages/llm/src/protocols/shared.ts (`parseToolInput`)
pub fn parse_tool_input(route: &str, name: &str, raw: &str) -> Result<Value, LlmError> {
    parse_json(route, if raw.is_empty() { "{}" } else { raw }, &format!("Invalid JSON input for {} tool call {}", route, name))
}

/// `ProviderShared.parseJson(route, input, message)`.
/// From reference/packages/llm/src/protocols/shared.ts (`parseJson`)
pub fn parse_json(route: &str, input: &str, message: &str) -> Result<Value, LlmError> {
    decode_json(input).map_err(|_| LlmError::event_error(route, message, Some(input.to_string())))
}

/// `ProviderShared.eventError(route, message, raw)`.
pub fn event_error(route: &str, message: impl Into<String>, raw: Option<String>) -> LlmError {
    LlmError::event_error(route, message, raw)
}

/// `ProviderShared.invalidRequest(message)`.
pub fn invalid_request(message: impl Into<String>) -> LlmError {
    LlmError::invalid_request(message)
}

/// `ProviderShared.errorText(error)`.
/// From reference/packages/llm/src/protocols/shared.ts (`errorText`)
pub fn error_text(error: &anyhow::Error) -> String {
    error.to_string()
}

pub fn error_text_str(error: &str) -> String {
    error.to_string()
}

pub fn error_text_value(error: &Value) -> String {
    match error {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => "Unknown stream error".to_string(),
    }
}

pub const IMAGE_MIMES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];
pub const VIDEO_MIMES: [&str; 3] = ["video/mp4", "video/webm", "video/quicktime"];
pub const AUDIO_MIMES: [&str; 6] = ["audio/wav", "audio/mp3", "audio/aiff", "audio/aac", "audio/ogg", "audio/flac"];
pub const MEDIA_MIMES: [&str; 13] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "video/mp4",
    "video/webm",
    "video/quicktime",
    "audio/wav",
    "audio/mp3",
    "audio/aiff",
    "audio/aac",
    "audio/ogg",
    "audio/flac",
];

pub const MAX_MEDIA_ENCODED_BYTES: usize = 28 * 1024 * 1024;
pub const MAX_MEDIA_DECODED_BYTES: usize = 20 * 1024 * 1024;

/// `ValidatedMedia`.
/// From reference/packages/llm/src/protocols/shared.ts (`ValidatedMedia`)
#[derive(Debug, Clone)]
pub struct ValidatedMedia {
    pub mime: String,
    pub base64: String,
    pub data_url: String,
    pub bytes: Vec<u8>,
}

const BASE64_PATTERN: &str = r"^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$";

/// `ProviderShared.validateMedia(route, part, supportedMimes)`.
/// From reference/packages/llm/src/protocols/shared.ts (`validateMedia`)
pub fn validate_media(route: &str, part: &MediaPart, supported_mimes: &std::collections::HashSet<String>) -> Result<ValidatedMedia, LlmError> {
    let mime = part.media_type.to_lowercase();
    if !supported_mimes.contains(&mime) {
        return Err(invalid_request(format!("{} does not support media type {}", route, part.media_type)));
    }

    let base64 = match &part.data {
        MediaData::Bytes(bytes) => {
            if bytes.len() > MAX_MEDIA_DECODED_BYTES {
                return Err(invalid_request(format!(
                    "{} media exceeds the {} byte decoded limit",
                    route, MAX_MEDIA_DECODED_BYTES
                )));
            }
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
        MediaData::Base64(data) => {
            if let Some(rest) = data.strip_prefix("data:") {
                let regex = regex::Regex::new(r"^data:([^;,]+);base64,([A-Za-z0-9+/]*={0,2})$").unwrap();
                let captures = regex.captures(rest).ok_or_else(|| {
                    invalid_request(format!("{} media data URL must contain valid base64", route))
                })?;
                let data_mime = captures.get(1).map(|m| m.as_str().to_lowercase()).unwrap_or_default();
                if data_mime != mime {
                    return Err(invalid_request(format!(
                        "{} media type {} does not match data URL type {}",
                        route, part.media_type, data_mime
                    )));
                }
                captures.get(2).map(|m| m.as_str().to_string()).unwrap_or_default()
            } else {
                data.clone()
            }
        }
    };

    if base64.len() > MAX_MEDIA_ENCODED_BYTES {
        return Err(invalid_request(format!("{} media exceeds the {} byte encoded limit", route, MAX_MEDIA_ENCODED_BYTES)));
    }
    if base64.is_empty() || base64.len() % 4 != 0 {
        return Err(invalid_request(format!("{} media must contain valid base64", route)));
    }
    let pattern = regex::Regex::new(BASE64_PATTERN).unwrap();
    if !pattern.is_match(&base64) {
        return Err(invalid_request(format!("{} media must contain valid base64", route)));
    }
    use base64::Engine as _;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(&base64) {
        Ok(bytes) => bytes,
        Err(_) => return Err(invalid_request(format!("{} media must contain valid base64", route))),
    };
    if bytes.len() > MAX_MEDIA_DECODED_BYTES {
        return Err(invalid_request(format!("{} media exceeds the {} byte decoded limit", route, MAX_MEDIA_DECODED_BYTES)));
    }
    if base64::engine::general_purpose::STANDARD.encode(&bytes) != base64 {
        return Err(invalid_request(format!("{} media must contain canonical base64", route)));
    }
    Ok(ValidatedMedia {
        mime: mime.clone(),
        data_url: format!("data:{};base64,{}", mime, base64),
        base64,
        bytes,
    })
}

/// `ProviderShared.validateToolFile(route, part, supportedMimes)`.
/// From reference/packages/llm/src/protocols/shared.ts (`validateToolFile`)
pub fn validate_tool_file(route: &str, part: &ToolFileContent, supported_mimes: &std::collections::HashSet<String>) -> Result<ValidatedMedia, LlmError> {
    validate_media(
        route,
        &MediaPart { part_type: "media".to_string(), media_type: part.mime.clone(), data: MediaData::Base64(part.uri.clone()), filename: part.name.clone(), metadata: None },
        supported_mimes,
    )
}

/// `ProviderShared.trimBaseUrl(value)`.
/// From reference/packages/llm/src/protocols/shared.ts (`trimBaseUrl`)
pub fn trim_base_url(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}

/// `ProviderShared.toolResultText(part)`.
/// From reference/packages/llm/src/protocols/shared.ts (`toolResultText`)
pub fn tool_result_text(part: &ToolResultPart) -> String {
    match &part.result {
        ToolResultValue::Text { value } => match value {
            Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
        },
        ToolResultValue::Error { value } => {
            let structured = !value.is_array()
                && (value.is_object() || value.is_null());
            if structured {
                encode_json(value)
            } else {
                match value {
                    Value::String(s) => s.clone(),
                    other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
                }
            }
        }
        other => encode_json(&match other {
            ToolResultValue::Json { value } => value.clone(),
            ToolResultValue::Content { value } => serde_json::to_value(value).unwrap_or(Value::Null),
            ToolResultValue::Text { value } | ToolResultValue::Error { value } => value.clone(),
        }),
    }
}

/// `totalTokens` policy.
/// From reference/packages/llm/src/protocols/shared.ts (`totalTokens`)
pub fn total_tokens(input_tokens: Option<i64>, output_tokens: Option<i64>, total: Option<i64>) -> Option<i64> {
    if let Some(total) = total {
        return Some(total);
    }
    if input_tokens.is_none() && output_tokens.is_none() {
        return None;
    }
    Some(input_tokens.unwrap_or(0) + output_tokens.unwrap_or(0))
}

/// `subtractTokens` — clamped difference.
/// From reference/packages/llm/src/protocols/shared.ts (`subtractTokens`)
pub fn subtract_tokens(total: Option<i64>, subtrahend: Option<i64>) -> Option<i64> {
    match total {
        None => None,
        Some(total) => match subtrahend {
            None => Some(total),
            Some(subtrahend) => Some((total - subtrahend).max(0)),
        },
    }
}

/// `sumTokens`.
/// From reference/packages/llm/src/protocols/shared.ts (`sumTokens`)
pub fn sum_tokens(values: &[Option<i64>]) -> Option<i64> {
    if values.iter().all(|value| value.is_none()) {
        return None;
    }
    Some(values.iter().map(|value| value.unwrap_or(0)).sum())
}

/// `formatContentTypes` — "a, b, and c".
/// From reference/packages/llm/src/protocols/shared.ts (`unsupportedContent`)
fn format_content_types(types: &[&str]) -> String {
    match types {
        [] => String::new(),
        [one] => (*one).to_string(),
        [a, b] => format!("{} and {}", a, b),
        many => format!("{}, and {}", many[..many.len() - 1].join(", "), many[many.len() - 1]),
    }
}

/// `ProviderShared.supportsContent(part, types)`.
pub fn supports_content(part: &ContentPart, types: &[&str]) -> bool {
    types.contains(&part.kind())
}

/// `ProviderShared.unsupportedContent(route, role, types)`.
/// From reference/packages/llm/src/protocols/shared.ts (`unsupportedContent`)
pub fn unsupported_content(route: &str, role: &str, types: &[&str], detail: String) -> LlmError {
    invalid_request(format!("{} {} messages {} {}", route, role, detail, format_content_types(types)))
}

/// Convenience overload for the standard message.
pub fn unsupported(route: &str, role: &str, types: &[&str]) -> LlmError {
    invalid_request(format!("{} {} messages only support {} content for now", route, role, format_content_types(types)))
}

/// `matchToolChoice` — dispatch over the tool-choice mode.
/// From reference/packages/llm/src/protocols/shared.ts (`matchToolChoice`)
pub enum ToolChoiceLowering<Auto, None, Required, Tool> {
    Auto(Auto),
    None(None),
    Required(Required),
    Tool(Tool),
}

pub fn match_tool_choice<Auto, None, Required, Tool>(
    route: &str,
    tool_choice: &crate::schema::ToolChoice,
    cases: MatchToolChoiceCases<Auto, None, Required, Tool>,
) -> Result<ToolChoiceLowering<Auto, None, Required, Tool>, LlmError> {
    match tool_choice.kind {
        crate::schema::ToolChoiceType::Auto => Ok(ToolChoiceLowering::Auto((cases.auto)())),
        crate::schema::ToolChoiceType::None => Ok(ToolChoiceLowering::None((cases.none)())),
        crate::schema::ToolChoiceType::Required => Ok(ToolChoiceLowering::Required((cases.required)())),
        crate::schema::ToolChoiceType::Tool => {
            let Some(name) = &tool_choice.name else {
                return Err(invalid_request(format!("{} tool choice requires a tool name", route)));
            };
            Ok(ToolChoiceLowering::Tool((cases.tool)(name)))
        }
    }
}

pub struct MatchToolChoiceCases<Auto, None, Required, Tool> {
    pub auto: Box<dyn Fn() -> Auto>,
    pub none: Box<dyn Fn() -> None>,
    pub required: Box<dyn Fn() -> Required>,
    pub tool: Box<dyn Fn(&str) -> Tool>,
}

/// `validateWith(decoder)` — map decode errors to `InvalidRequest`.
/// From reference/packages/llm/src/protocols/shared.ts (`validateWith`)
pub fn validate_with<A, E: std::fmt::Display>(decode: impl FnOnce(&str) -> Result<A, E>) -> impl FnOnce(&str) -> Result<A, LlmError> {
    move |input| decode(input).map_err(|error| invalid_request(error.to_string()))
}

/// `JsonObject` — `Record<string, unknown>`.
pub type JsonObject = serde_json::Value;

/// Convenience accessor: optional array field, defaults to `None`.
pub fn optional_array(value: Option<Vec<Value>>) -> Option<Vec<Value>> {
    value
}

/// `systemPartText` — join system part text.
pub fn system_part_text(system: &[SystemPart]) -> String {
    system.iter().map(|part| part.text.as_str()).collect::<Vec<_>>().join("\n")
}

/// `totalTokens` shorthand used by mappers.
pub fn usage_total_tokens(input: Option<i64>, output: Option<i64>, total: Option<i64>) -> Option<i64> {
    total_tokens(input, output, total)
}

/// Route key string: `provider/route`.
pub fn route_key(provider: &str, route: &str) -> String {
    format!("{}/{}", provider, route)
}

/// Look up a `BTreeMap`-shaped provider metadata value.
pub fn provider_record<'a>(metadata: &'a crate::schema::ProviderMetadata, key: &str) -> Option<&'a serde_json::Map<String, Value>> {
    metadata.get(key)
}
