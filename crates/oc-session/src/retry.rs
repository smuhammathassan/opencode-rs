/// From reference/packages/opencode/src/session/retry.ts
///
/// Retry delay computation and retryability classification for LLM errors.
use crate::v1::Error;

pub const GO_UPSELL_MESSAGE: &str = "Free usage exceeded, subscribe to Go";
pub const GO_UPSELL_URL: &str = "https://opencode.ai/go";

pub const RETRY_INITIAL_DELAY: u64 = 2000;
pub const RETRY_BACKOFF_FACTOR: u64 = 2;
pub const RETRY_MAX_DELAY_NO_HEADERS: u64 = 30_000;
pub const RETRY_MAX_DELAY: u64 = 2_147_483_647;

#[derive(Debug, Clone)]
pub struct RetryAction {
    pub reason: String,
    pub provider: String,
    pub title: String,
    pub message: String,
    pub label: String,
    pub link: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Retryable {
    pub message: String,
    pub action: Option<RetryAction>,
}

fn cap(ms: u64) -> u64 {
    ms.min(RETRY_MAX_DELAY)
}

fn backoff(attempt: u64) -> u64 {
    RETRY_INITIAL_DELAY
        .saturating_mul(RETRY_BACKOFF_FACTOR.saturating_pow((attempt.saturating_sub(1)) as u32))
}

/// From reference `retry.ts:delay`.
pub fn delay(attempt: u64, error: Option<&Error>) -> u64 {
    if let Some(Error::ApiError {
        response_headers: Some(headers),
        ..
    }) = error
    {
        if let Some(retry_after_ms) = headers.get("retry-after-ms").and_then(|v| v.as_str()) {
            if let Ok(parsed_ms) = retry_after_ms.parse::<f64>() {
                if parsed_ms.is_finite() {
                    return cap(parsed_ms.max(0.0) as u64);
                }
            }
        }
        if let Some(retry_after) = headers.get("retry-after").and_then(|v| v.as_str()) {
            if let Ok(parsed_seconds) = retry_after.parse::<f64>() {
                if parsed_seconds.is_finite() {
                    return cap((parsed_seconds * 1000.0).ceil() as u64);
                }
            }
            let parsed_date = http_date_seconds_since_epoch(retry_after);
            if parsed_date > 0 {
                return cap(parsed_date);
            }
        }
        return cap(backoff(attempt));
    }
    cap(backoff(attempt).min(RETRY_MAX_DELAY_NO_HEADERS))
}

/// Best-effort parse of an HTTP-date (RFC 7231) into ms until it elapses.
fn http_date_seconds_since_epoch(value: &str) -> u64 {
    let parsed = crate::util::http_date_to_unix_millis(value);
    if parsed == 0 {
        return 0;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let diff = parsed as i64 - now as i64;
    if diff > 0 {
        diff as u64
    } else {
        0
    }
}

/// From reference `retry.ts:retryable`.
pub fn retryable(error: &Error, provider: &str) -> Option<Retryable> {
    // context overflow errors should not be retried
    if matches!(error, Error::ContextOverflowError { .. }) {
        return None;
    }
    if let Error::ApiError {
        response_body,
        status_code,
        is_retryable,
        response_headers,
        message,
        ..
    } = error
    {
        let status = *status_code;
        // 5xx errors are transient server failures and should always be retried
        if !is_retryable && !(status.is_some() && status.unwrap() >= 500) {
            return None;
        }
        if let Some(body) = response_body {
            if body.contains("FreeUsageLimitError") {
                return Some(Retryable {
                    message: GO_UPSELL_MESSAGE.to_string(),
                    action: Some(RetryAction {
                        reason: "free_tier_limit".to_string(),
                        provider: provider.to_string(),
                        title: "Free limit reached".to_string(),
                        message: "Subscribe to OpenCode Go for reliable access to the best open-source models, starting at $5/month.".to_string(),
                        label: "subscribe".to_string(),
                        link: Some(GO_UPSELL_URL.to_string()),
                    }),
                });
            }
            if body.contains("GoUsageLimitError") {
                let parsed = parse_json(body.as_str());
                let workspace = str_value(parsed.as_ref(), &["metadata", "workspace"]);
                let limit_name = str_value(parsed.as_ref(), &["metadata", "limitName"]);
                let retry_after = response_headers
                    .as_ref()
                    .and_then(|headers| headers.get("retry-after"))
                    .and_then(|v| v.as_str())
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite());
                let reset_in = match retry_after {
                    None => String::new(),
                    Some(raw) => {
                        let seconds = (raw.max(0.0).ceil()) as u64;
                        let days = seconds / 86_400;
                        let hours = (seconds % 86_400) / 3_600;
                        let minutes = ((seconds % 3_600) as f64 / 60.0).ceil() as u64;
                        let unit = |value: u64, name: &str| {
                            format!("{value} {name}{}", if value == 1 { "" } else { "s" })
                        };
                        if days > 0 {
                            if hours > 0 {
                                format!("{} {}", unit(days, "day"), unit(hours, "hour"))
                            } else {
                                unit(days, "day")
                            }
                        } else if hours > 0 {
                            if minutes > 0 {
                                format!("{} {}", unit(hours, "hour"), unit(minutes, "minute"))
                            } else {
                                unit(hours, "hour")
                            }
                        } else if minutes > 0 {
                            unit(minutes, "minute")
                        } else {
                            "less than a minute".to_string()
                        }
                    }
                };

                let name = if limit_name.is_empty() {
                    "Usage limit".to_string()
                } else {
                    format!("{limit_name} usage limit")
                };
                let message = format!(
                    "{name} reached. It will reset in {reset_in}. To continue using this model now, enable usage from your available balance"
                );
                let link = format!("https://opencode.ai/workspace/{workspace}/go");
                return Some(Retryable {
                    message: format!("{message} - {link}"),
                    action: Some(RetryAction {
                        reason: "account_rate_limit".to_string(),
                        provider: provider.to_string(),
                        title: "Go limit reached".to_string(),
                        message,
                        label: "open settings".to_string(),
                        link: Some(link),
                    }),
                });
            }
        }
        let message = if message.contains("Overloaded") {
            "Provider is overloaded"
        } else {
            message.as_str()
        };
        return Some(Retryable {
            message: message.to_string(),
            action: None,
        });
    }

    // Check for rate limit patterns in plain text error messages
    let msg = match error {
        Error::UnknownError { message, .. }
        | Error::AuthError { message, .. }
        | Error::AbortedError { message }
        | Error::StructuredOutputError { message, .. } => Some(message.as_str()),
        Error::ContentFilterError { message } => Some(message.as_str()),
        Error::OutputLengthError | Error::ContextOverflowError { .. } | Error::ApiError { .. } => {
            None
        }
    };
    if let Some(msg) = msg {
        let lower = msg.to_lowercase();
        if lower.contains("rate increased too quickly")
            || lower.contains("rate limit")
            || lower.contains("too many requests")
        {
            return Some(Retryable {
                message: msg.to_string(),
                action: None,
            });
        }
        let json = parse_json(msg);
        if let Some(serde_json::Value::Object(map)) = json {
            let code = map.get("code").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(error) = map.get("error") {
                if map.get("type").and_then(|v| v.as_str()) == Some("error")
                    && error.get("type").and_then(|v| v.as_str()) == Some("too_many_requests")
                {
                    return Some(Retryable {
                        message: "Too Many Requests".to_string(),
                        action: None,
                    });
                }
            }
            if code.contains("exhausted") || code.contains("unavailable") {
                return Some(Retryable {
                    message: "Provider is overloaded".to_string(),
                    action: None,
                });
            }
            if let Some(error) = map.get("error") {
                if map.get("type").and_then(|v| v.as_str()) == Some("error")
                    && error
                        .get("code")
                        .and_then(|v| v.as_str())
                        .is_some_and(|code| code.contains("rate_limit"))
                {
                    return Some(Retryable {
                        message: "Rate Limited".to_string(),
                        action: None,
                    });
                }
            }
        }
    }
    None
}
fn str_value(parsed: Option<&serde_json::Value>, path: &[&str]) -> String {
    let mut current = parsed;
    for key in path {
        match current {
            Some(serde_json::Value::Object(map)) => current = map.get(*key),
            _ => return String::new(),
        }
    }
    match current {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn parse_json(value: &str) -> Option<serde_json::Value> {
    serde_json::from_str(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JsonMap;
    use serde_json::json;

    fn api_error(
        message: &str,
        is_retryable: bool,
        status: Option<u64>,
        body: Option<&str>,
        headers: Option<JsonMap>,
    ) -> Error {
        Error::ApiError {
            message: message.to_string(),
            status_code: status,
            is_retryable,
            response_headers: headers,
            response_body: body.map(|s| s.to_string()),
            metadata: None,
        }
    }

    #[test]
    fn non_retryable_4xx_is_not_retried() {
        let err = api_error("bad request", false, Some(400), None, None);
        assert!(retryable(&err, "openai").is_none());
    }

    #[test]
    fn context_overflow_is_not_retried() {
        let err = Error::ContextOverflowError {
            message: "context".into(),
            response_body: None,
        };
        assert!(retryable(&err, "openai").is_none());
    }

    #[test]
    fn server_error_always_retried() {
        let err = api_error("server error", false, Some(500), None, None);
        let retry = retryable(&err, "openai").unwrap();
        assert!(retry.action.is_none());
        assert_eq!(retry.message, "server error");
    }

    #[test]
    fn overloaded_message_rewritten() {
        let err = api_error("Overloaded error from provider", true, None, None, None);
        let retry = retryable(&err, "openai").unwrap();
        assert_eq!(retry.message, "Provider is overloaded");
    }

    #[test]
    fn free_tier_upsell() {
        let err = api_error("nope", true, None, Some("FreeUsageLimitError boom"), None);
        let retry = retryable(&err, "openai").unwrap();
        assert_eq!(retry.message, GO_UPSELL_MESSAGE);
        assert_eq!(retry.action.unwrap().label, "subscribe");
    }

    #[test]
    fn go_usage_limit_message() {
        let body = json!({
            "type": "error",
            "error": { "code": "GoUsageLimitError" },
            "metadata": { "workspace": "ws1", "limitName": "Pro" }
        })
        .to_string();
        let err = api_error("nope", true, None, Some(&body), None);
        let retry = retryable(&err, "openai").unwrap();
        let action = retry.action.unwrap();
        assert_eq!(action.reason, "account_rate_limit");
        assert!(retry
            .message
            .contains("https://opencode.ai/workspace/ws1/go"));
    }

    #[test]
    fn backoff_capped_without_headers() {
        assert_eq!(delay(1, None), 2000);
        assert_eq!(delay(2, None), 4000);
        assert_eq!(delay(3, None), 8000);
        assert_eq!(delay(20, None), RETRY_MAX_DELAY_NO_HEADERS);
    }

    #[test]
    fn retry_after_header_used() {
        let mut headers = JsonMap::new();
        headers.insert("retry-after".into(), json!("120"));
        let err = api_error("busy", true, Some(429), None, Some(headers));
        assert_eq!(delay(1, Some(&err)), 120_000);
    }
}
