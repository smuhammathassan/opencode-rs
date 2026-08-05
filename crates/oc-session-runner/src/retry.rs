//! Session retry policy with exponential backoff and error mapping.
//!
//! Ports `packages/opencode/src/session/retry.ts`. Context overflow failures
//! are never retried; provider 5xx and rate-limit errors are; the delay honors
//! `retry-after` headers when present, otherwise back off exponentially.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// `GO_UPSELL_MESSAGE`
/// /// From reference/packages/opencode/src/session/retry.ts
pub const GO_UPSELL_MESSAGE: &str = "Free usage exceeded, subscribe to Go";
/// `GO_UPSELL_URL`
/// /// From reference/packages/opencode/src/session/retry.ts
pub const GO_UPSELL_URL: &str = "https://opencode.ai/go";

/// `RetryReason`
/// /// From reference/packages/opencode/src/session/retry.ts
#[derive(Debug, Clone, PartialEq)]
pub enum RetryReason {
    FreeTierLimit,
    AccountRateLimit,
    Other(String),
}

impl RetryReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::FreeTierLimit => "free_tier_limit",
            Self::AccountRateLimit => "account_rate_limit",
            Self::Other(value) => value,
        }
    }
}

/// `Retryable` — a failure that warrants a retry plus the surfaced message.
/// /// From reference/packages/opencode/src/session/retry.ts
#[derive(Debug, Clone, PartialEq)]
pub struct Retryable {
    pub message: String,
    pub action: Option<RetryAction>,
}

/// `Retryable["action"]`
/// /// From reference/packages/opencode/src/session/retry.ts
#[derive(Debug, Clone, PartialEq)]
pub struct RetryAction {
    pub reason: RetryReason,
    pub provider: String,
    pub title: String,
    pub message: String,
    pub label: String,
    pub link: Option<String>,
}

/// Constants from the reference retry schedule.
/// /// From reference/packages/opencode/src/session/retry.ts
pub const RETRY_INITIAL_DELAY_MS: u64 = 2000;
pub const RETRY_BACKOFF_FACTOR: u64 = 2;
pub const RETRY_MAX_DELAY_NO_HEADERS_MS: u64 = 30_000;
pub const RETRY_MAX_DELAY_MS: u64 = 2_147_483_647;

/// Mirror of `SessionV1.APIError["data"]`.
/// /// From reference/packages/core/src/v1/session.ts
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApiErrorData {
    pub status_code: Option<f64>,
    pub is_retryable: bool,
    pub response_body: Option<String>,
    pub response_headers: Option<HashMap<String, String>>,
    pub message: String,
    pub metadata: Option<HashMap<String, String>>,
}

/// Mirror of `SessionV1.APIError`.
/// /// From reference/packages/core/src/v1/session.ts
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub data: ApiErrorData,
}

/// The normalized error shape fed to `retryable`. `ContextOverflow` mirrors
/// `SessionV1.ContextOverflowError`.
/// /// From reference/packages/opencode/src/session/retry.ts (`Err`)
#[derive(Debug, Clone, PartialEq)]
pub enum Err {
    Api(ApiError),
    ContextOverflow,
    Other { message: Option<String> },
}

impl Err {
    pub fn api_error(&self) -> Option<&ApiError> {
        match self {
            Self::Api(error) => Some(error),
            _ => None,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Api(error) => Some(&error.data.message),
            Self::Other { message } => message.as_deref(),
            Self::ContextOverflow => None,
        }
    }
}

fn cap(ms: u64) -> u64 {
    ms.min(RETRY_MAX_DELAY_MS)
}

fn pow(base: u64, exponent: u32) -> u64 {
    if exponent == 0 {
        return 1;
    }
    base.saturating_pow(exponent)
}

/// `RETRY_INITIAL_DELAY * BACKOFF^(attempt - 1)`. Attempt is 0-based, so the
/// first failure waits half the initial delay (matching `Math.pow` with a
/// negative exponent).
/// /// From reference/packages/opencode/src/session/retry.ts
fn backoff_delay(attempt: u32) -> u64 {
    match attempt {
        0 => RETRY_INITIAL_DELAY_MS / 2,
        attempt => RETRY_INITIAL_DELAY_MS * pow(RETRY_BACKOFF_FACTOR, attempt - 1),
    }
}

/// Retry delay for an attempt (0-based, matching `Schedule` metadata).
/// Respects `retry-after` response headers; otherwise exponential backoff.
/// /// From reference/packages/opencode/src/session/retry.ts (`delay`)
pub fn delay(attempt: u32, error: Option<&ApiError>) -> u64 {
    if let Some(error) = error {
        if let Some(headers) = &error.data.response_headers {
            if let Some(retry_after_ms) = headers.get("retry-after-ms") {
                if let Ok(parsed) = retry_after_ms.parse::<f64>() {
                    if parsed.is_finite() {
                        return cap(parsed.max(0.0) as u64);
                    }
                }
            }
            if let Some(retry_after) = headers.get("retry-after") {
                if let Ok(parsed_seconds) = retry_after.parse::<f64>() {
                    if parsed_seconds.is_finite() {
                        return cap((parsed_seconds.max(0.0) * 1000.0).ceil() as u64);
                    }
                }
                // Try parsing as an HTTP date.
                if let Ok(parsed_date) = chrono::DateTime::parse_from_rfc2822(retry_after) {
                    let now = chrono::Utc::now();
                    let parsed = parsed_date.with_timezone(&chrono::Utc);
                    let delta = (parsed - now).num_milliseconds();
                    if delta > 0 {
                        return cap(delta as u64);
                    }
                }
            }
            return cap(backoff_delay(attempt));
        }
    }
    cap(backoff_delay(attempt).min(RETRY_MAX_DELAY_NO_HEADERS_MS))
}

fn str(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn parse_json(value: Option<&str>) -> Option<serde_json::Value> {
    serde_json::from_str(value?).ok()
}

/// Decide whether an error is retryable and what to surface.
/// /// From reference/packages/opencode/src/session/retry.ts (`retryable`)
pub fn retryable(error: &Err, provider: &str) -> Option<Retryable> {
    if matches!(error, Err::ContextOverflow) {
        return None;
    }
    if let Err::Api(error) = error {
        let status = error.data.status_code;
        // 5xx errors are transient server failures and should always be
        // retried, even when the provider SDK doesn't mark them retryable.
        if !error.data.is_retryable && !(status.is_some_and(|status| status >= 500.0)) {
            return None;
        }
        if let Some(body) = &error.data.response_body {
            if body.contains("FreeUsageLimitError") {
                return Some(Retryable {
                    message: GO_UPSELL_MESSAGE.to_string(),
                    action: Some(RetryAction {
                        reason: RetryReason::FreeTierLimit,
                        provider: provider.to_string(),
                        title: "Free limit reached".into(),
                        message: "Subscribe to OpenCode Go for reliable access to the best open-source models, starting at $5/month.".into(),
                        label: "subscribe".into(),
                        link: Some(GO_UPSELL_URL.to_string()),
                    }),
                });
            }
            if body.contains("GoUsageLimitError") {
                let parsed = parse_json(Some(body));
                let metadata = parsed.as_ref().and_then(|value| value.get("metadata"));
                let workspace = metadata
                    .and_then(|value| value.get("workspace"))
                    .map(str)
                    .unwrap_or_default();
                let limit_name = metadata
                    .and_then(|value| value.get("limitName"))
                    .map(str)
                    .unwrap_or_default();
                let retry_after = error
                    .data
                    .response_headers
                    .as_ref()
                    .and_then(|headers| headers.get("retry-after"))
                    .and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite());
                let reset_in = match retry_after {
                    None => String::new(),
                    Some(seconds) => {
                        let seconds = seconds.max(0.0).ceil() as i64;
                        let days = seconds / 86_400;
                        let hours = (seconds % 86_400) / 3_600;
                        let minutes = ((seconds % 3_600) + 59) / 60;
                        let unit = |value: i64, name: &str| {
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
                            "less than a minute".into()
                        }
                    }
                };
                let message = format!(
                    "{} reached. It will reset in {}. To continue using this model now, enable usage from your available balance",
                    if limit_name.is_empty() {
                        "Usage limit".to_string()
                    } else {
                        format!("{limit_name} usage limit")
                    },
                    reset_in,
                );
                let link = format!("https://opencode.ai/workspace/{workspace}/go");
                return Some(Retryable {
                    message: format!("{message} - {link}"),
                    action: Some(RetryAction {
                        reason: RetryReason::AccountRateLimit,
                        provider: provider.to_string(),
                        title: "Go limit reached".into(),
                        message,
                        label: "open settings".into(),
                        link: Some(link),
                    }),
                });
            }
        }
        return Some(Retryable {
            message: if error.data.message.contains("Overloaded") {
                "Provider is overloaded".to_string()
            } else {
                error.data.message.clone()
            },
            action: None,
        });
    }

    // Check for rate limit patterns in plain text error messages.
    if let Some(message) = error.message() {
        let lower = message.to_lowercase();
        if lower.contains("rate increased too quickly")
            || lower.contains("rate limit")
            || lower.contains("too many requests")
        {
            return Some(Retryable {
                message: message.to_string(),
                action: None,
            });
        }
        if let Some(json) = parse_json(Some(message)) {
            let code = json
                .get("code")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            if json.get("type").and_then(|value| value.as_str()) == Some("error")
                && json
                    .get("error")
                    .and_then(|value| value.get("type"))
                    .and_then(|value| value.as_str())
                    == Some("too_many_requests")
            {
                return Some(Retryable {
                    message: "Too Many Requests".into(),
                    action: None,
                });
            }
            if code.contains("exhausted") || code.contains("unavailable") {
                return Some(Retryable {
                    message: "Provider is overloaded".into(),
                    action: None,
                });
            }
            if json.get("type").and_then(|value| value.as_str()) == Some("error")
                && json
                    .get("error")
                    .and_then(|value| value.get("code"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|code| code.contains("rate_limit"))
            {
                return Some(Retryable {
                    message: "Rate Limited".into(),
                    action: None,
                });
            }
        }
    }
    None
}

/// `RetryInfo` handed to the `set` callback.
/// /// From reference/packages/opencode/src/session/retry.ts (`policy`)
#[derive(Debug, Clone)]
pub struct RetryInfo {
    pub attempt: u32,
    pub message: String,
    pub action: Option<RetryAction>,
    pub next: i64,
}

/// A retry policy: given a failure and a 0-based attempt, decide the wait and
/// notify the status callback. `None` means "do not retry".
/// /// From reference/packages/opencode/src/session/retry.ts (`policy`)
pub struct RetryPolicy {
    provider: String,
    parse: Arc<dyn Fn(&dyn std::error::Error) -> Err + Send + Sync + 'static>,
    set: Arc<dyn Fn(RetryInfo) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static>,
}

impl RetryPolicy {
    pub fn new<P, S>(provider: impl Into<String>, parse: P, set: S) -> Self
    where
        P: Fn(&dyn std::error::Error) -> Err + Send + Sync + 'static,
        S: Fn(RetryInfo) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
    {
        Self {
            provider: provider.into(),
            parse: Arc::new(parse),
            set: Arc::new(set),
        }
    }

    /// Evaluate one failure. On a retryable error, records the retry through
    /// `set` and returns the backoff wait; otherwise returns `None`.
    /// /// From reference/packages/opencode/src/session/retry.ts (`policy`)
    pub async fn on_failure(
        &self,
        error: &dyn std::error::Error,
        attempt: u32,
        now_ms: i64,
    ) -> Option<Duration> {
        let parsed = (self.parse)(error);
        let retry = retryable(&parsed, &self.provider)?;
        let wait = delay(attempt, parsed.api_error());
        let next = now_ms + wait as i64;
        (self.set)(RetryInfo {
            attempt,
            message: retry.message,
            action: retry.action,
            next,
        })
        .await;
        Some(Duration::from_millis(wait))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn api_error(status: Option<f64>, is_retryable: bool, body: Option<&str>) -> ApiError {
        ApiError {
            data: ApiErrorData {
                status_code: status,
                is_retryable,
                response_body: body.map(|body| body.to_string()),
                message: "boom".into(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn context_overflow_is_not_retryable() {
        assert_eq!(retryable(&Err::ContextOverflow, "openai"), None);
    }

    #[test]
    fn non_retryable_4xx_is_not_retried() {
        let error = Err::Api(api_error(Some(400.0), false, None));
        assert_eq!(retryable(&error, "openai"), None);
    }

    #[test]
    fn retryable_4xx_is_retried() {
        let error = Err::Api(api_error(Some(429.0), true, None));
        assert_eq!(retryable(&error, "openai").unwrap().message, "boom");
    }

    #[test]
    fn any_5xx_is_retried_even_when_not_marked() {
        let error = Err::Api(api_error(Some(502.0), false, None));
        assert!(retryable(&error, "openai").is_some());
    }

    #[test]
    fn overloaded_message_is_mapped() {
        let error = Err::Api(ApiError {
            data: ApiErrorData {
                status_code: Some(503.0),
                is_retryable: false,
                message: "Overloaded".into(),
                ..Default::default()
            },
        });
        assert_eq!(
            retryable(&error, "openai").unwrap().message,
            "Provider is overloaded"
        );
    }

    #[test]
    fn free_usage_limit_upsells() {
        let error = Err::Api(api_error(Some(429.0), true, Some("FreeUsageLimitError")));
        let retry = retryable(&error, "openai").unwrap();
        assert_eq!(retry.message, GO_UPSELL_MESSAGE);
        let action = retry.action.unwrap();
        assert_eq!(action.reason, RetryReason::FreeTierLimit);
        assert_eq!(action.link.as_deref(), Some(GO_UPSELL_URL));
    }

    #[test]
    fn plain_rate_limit_pattern_is_retried() {
        let error = Err::Other {
            message: Some("Rate limit exceeded for requests".into()),
        };
        assert_eq!(
            retryable(&error, "openai").unwrap().message,
            "Rate limit exceeded for requests"
        );
    }

    #[test]
    fn too_many_requests_json_is_mapped() {
        let error = Err::Other {
            message: Some(
                json!({ "type": "error", "error": { "type": "too_many_requests" } }).to_string(),
            ),
        };
        assert_eq!(
            retryable(&error, "openai").unwrap().message,
            "Too Many Requests"
        );
    }

    #[test]
    fn exhausted_code_is_overloaded() {
        let error = Err::Other {
            message: Some(json!({ "code": "model_context_window_exhausted" }).to_string()),
        };
        assert_eq!(
            retryable(&error, "openai").unwrap().message,
            "Provider is overloaded"
        );
    }

    #[test]
    fn unknown_plain_error_is_not_retried() {
        let error = Err::Other {
            message: Some("random failure".into()),
        };
        assert_eq!(retryable(&error, "openai"), None);
    }

    #[test]
    fn delay_backs_off_exponentially_without_headers() {
        assert_eq!(delay(0, None), 1000);
        assert_eq!(delay(1, None), 2000);
        assert_eq!(delay(2, None), 4000);
        assert_eq!(delay(3, None), 8000);
    }

    #[test]
    fn delay_caps_without_headers() {
        assert_eq!(delay(10, None), RETRY_MAX_DELAY_NO_HEADERS_MS);
    }

    #[test]
    fn delay_honors_retry_after_ms_header() {
        let mut headers = HashMap::new();
        headers.insert("retry-after-ms".to_string(), "1500".to_string());
        let error = ApiError {
            data: ApiErrorData {
                response_headers: Some(headers),
                ..Default::default()
            },
        };
        assert_eq!(delay(0, Some(&error)), 1500);
    }

    #[test]
    fn delay_honors_retry_after_seconds_header() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "2".to_string());
        let error = ApiError {
            data: ApiErrorData {
                response_headers: Some(headers),
                ..Default::default()
            },
        };
        assert_eq!(delay(0, Some(&error)), 2000);
    }

    #[test]
    fn delay_caps_retry_after_to_max() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "99999999".to_string());
        let error = ApiError {
            data: ApiErrorData {
                response_headers: Some(headers),
                ..Default::default()
            },
        };
        assert_eq!(delay(0, Some(&error)), RETRY_MAX_DELAY_MS);
    }

    #[tokio::test]
    async fn policy_reports_and_returns_delay() {
        let policy = RetryPolicy::new(
            "openai",
            |_| Err::Other {
                message: Some("Rate limit".into()),
            },
            |info| {
                Box::pin(async move {
                    assert_eq!(info.attempt, 2);
                    assert_eq!(info.message, "Rate limit");
                })
            },
        );
        let error: Box<dyn std::error::Error> =
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, "rate limit"));
        let wait = policy.on_failure(error.as_ref(), 2, 1_000_000).await;
        assert!(wait.is_some());
    }
}
