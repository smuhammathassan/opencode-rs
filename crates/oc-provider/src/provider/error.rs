//! Provider request error normalization.
//!
//! From reference/packages/opencode/src/provider/error.ts.

use serde_json::Value;

/// `HeaderTimeoutError` from `error.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("Provider response headers timed out after {ms}ms")]
pub struct HeaderTimeoutError {
    pub ms: u64,
}

/// `ResponseStreamError` from `error.ts`.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ResponseStreamError(#[from] anyhow::Error);

/// A parsed error returned to callers.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParsedStreamError {
    ContextOverflow {
        message: String,
        #[serde(rename = "responseBody")]
        response_body: String,
    },
    ApiError {
        message: String,
        #[serde(rename = "isRetryable")]
        is_retryable: bool,
        #[serde(rename = "responseBody")]
        response_body: String,
    },
}

/// Parses a stream error from an arbitrary value.
///
/// From `parseStreamError()` in `error.ts`.
pub fn parse_stream_error(input: &Value) -> Option<ParsedStreamError> {
    let raw = json(input);
    let raw = raw?;
    let body = match raw.get("message").and_then(|m| m.as_str()) {
        Some(message) => serde_json::from_str::<Value>(message)
            .ok()
            .filter(|v| v.is_object())
            .unwrap_or_else(|| raw.clone()),
        None => raw.clone(),
    };
    let response_body = serde_json::to_string(&body).ok()?;
    if body.get("type").and_then(|v| v.as_str()) != Some("error") {
        return None;
    }
    let code = body
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_str());
    let error_message = || {
        body.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(str::to_string)
    };
    match code {
        Some("context_length_exceeded") => Some(ParsedStreamError::ContextOverflow {
            message: "Input exceeds context window of this model".to_string(),
            response_body,
        }),
        Some("insufficient_quota") => Some(ParsedStreamError::ApiError {
            message: "Quota exceeded. Check your plan and billing details.".to_string(),
            is_retryable: false,
            response_body,
        }),
        Some("usage_not_included") => Some(ParsedStreamError::ApiError {
            message: "To use Codex with your ChatGPT plan, upgrade to Plus: https://chatgpt.com/explore/plus."
                .to_string(),
            is_retryable: false,
            response_body,
        }),
        Some("invalid_prompt") => Some(ParsedStreamError::ApiError {
            message: error_message().unwrap_or_else(|| "Invalid prompt.".to_string()),
            is_retryable: false,
            response_body,
        }),
        Some("server_is_overloaded") | Some("server_error") => Some(ParsedStreamError::ApiError {
            message: error_message().unwrap_or_else(|| "Server error.".to_string()),
            is_retryable: true,
            response_body,
        }),
        _ => None,
    }
}

fn json(input: &Value) -> Option<Value> {
    match input {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(result) if result.is_object() => Some(result),
            _ => None,
        },
        value if value.is_object() => Some(value.clone()),
        _ => None,
    }
}

/// A parsed API call error returned to callers.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParsedApiCallError {
    ContextOverflow {
        message: String,
        #[serde(rename = "responseBody")]
        response_body: Option<String>,
    },
    ApiError {
        message: String,
        #[serde(rename = "statusCode")]
        status_code: Option<u16>,
        #[serde(rename = "isRetryable")]
        is_retryable: bool,
        #[serde(rename = "responseHeaders", skip_serializing_if = "Option::is_none")]
        response_headers: Option<std::collections::BTreeMap<String, String>>,
        #[serde(rename = "responseBody")]
        response_body: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<std::collections::BTreeMap<String, String>>,
    },
}

/// Input for `parse_api_call_error`.
#[derive(Debug, Clone)]
pub struct ApiCallErrorInput {
    pub provider_id: String,
    pub message: String,
    pub status_code: Option<u16>,
    pub is_retryable: bool,
    pub response_body: Option<String>,
    pub response_headers: Option<std::collections::BTreeMap<String, String>>,
    pub url: Option<String>,
}

fn is_context_overflow(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("context_length_exceeded")
        || lower.contains("token limit")
        || lower.contains("maximum context length")
        || lower.contains("input is too long")
        || lower.contains("context window")
}

/// Whether the provider error message should be enriched from the response
/// body. Mirrors `message()` in `error.ts`.
fn error_message(input: &ApiCallErrorInput, status_text: Option<&str>) -> String {
    let msg = input.message.trim();
    if msg.is_empty() {
        if let Some(body) = &input.response_body {
            if !body.is_empty() {
                return body.trim().to_string();
            }
        }
        if let Some(status_text) = status_text {
            if !status_text.is_empty() {
                return status_text.to_string();
            }
        }
        return "Unknown error".to_string();
    }

    let status_matches = match (&input.status_code, status_text) {
        (Some(code), Some(status_text)) => msg == status_text && *code > 0,
        _ => false,
    };
    let has_body = input.response_body.as_ref().is_some_and(|b| !b.is_empty());
    if !has_body || (input.status_code.is_some() && !status_matches) {
        return msg.to_string();
    }

    let body = input
        .response_body
        .as_deref()
        .and_then(|b| serde_json::from_str::<Value>(b).ok());
    if let Some(body) = body {
        let err_msg = body
            .get("message")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty());
        if let Some(err_msg) = err_msg {
            return format!("{}: {}", msg, err_msg);
        }
    }

    if let Some(body) = &input.response_body {
        if body.trim_start().to_lowercase().starts_with("<!doctype")
            || body.trim_start().to_lowercase().starts_with("<html")
        {
            match input.status_code {
                Some(401) => {
                    return "Unauthorized: request was blocked by a gateway or proxy. Your authentication token may be missing or expired — try running `opencode auth login <your provider URL>` to re-authenticate.".to_string()
                }
                Some(403) => {
                    return "Forbidden: request was blocked by a gateway or proxy. You may not have permission to access this resource — check your account and provider settings.".to_string()
                }
                _ => return msg.to_string(),
            }
        }
        return format!("{}: {}", msg, body);
    }

    msg.to_string()
}

/// Parses an API call error.
///
/// From `parseAPICallError()` in `error.ts`.
pub fn parse_api_call_error(input: &ApiCallErrorInput) -> ParsedApiCallError {
    const STATUS_CODES: &[(u16, &str)] = &[
        (400, "Bad Request"),
        (401, "Unauthorized"),
        (402, "Payment Required"),
        (403, "Forbidden"),
        (404, "Not Found"),
        (405, "Method Not Allowed"),
        (408, "Request Timeout"),
        (409, "Conflict"),
        (413, "Payload Too Large"),
        (415, "Unsupported Media Type"),
        (422, "Unprocessable Entity"),
        (429, "Too Many Requests"),
        (500, "Internal Server Error"),
        (501, "Not Implemented"),
        (502, "Bad Gateway"),
        (503, "Service Unavailable"),
        (504, "Gateway Timeout"),
    ];
    let status_text = input.status_code.and_then(|code| {
        STATUS_CODES
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, s)| *s)
    });

    let message = error_message(input, status_text);
    let body = input
        .response_body
        .as_deref()
        .and_then(|b| serde_json::from_str::<Value>(b).ok());

    let is_context_overflow = is_context_overflow(&message)
        || input.status_code == Some(413)
        || body.as_ref().is_some_and(|body| {
            body.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_str())
                == Some("context_length_exceeded")
        });
    if is_context_overflow {
        return ParsedApiCallError::ContextOverflow {
            message,
            response_body: input.response_body.clone(),
        };
    }

    let metadata = input.url.clone().map(|url| {
        let mut map = std::collections::BTreeMap::new();
        map.insert("url".to_string(), url);
        map
    });
    let is_openai = input.provider_id.starts_with("openai");
    let is_retryable = if is_openai {
        // openai sometimes returns 404 for models that are actually available
        input.status_code == Some(404) || input.is_retryable
    } else {
        input.is_retryable
    };
    ParsedApiCallError::ApiError {
        message,
        status_code: input.status_code,
        is_retryable,
        response_headers: input.response_headers.clone(),
        response_body: input.response_body.clone(),
        metadata,
    }
}
