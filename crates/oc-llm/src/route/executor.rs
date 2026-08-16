//! Request executor: HTTP error mapping + retry policy.
//! From reference/packages/llm/src/route/executor.ts

use std::collections::BTreeMap;

use reqwest::StatusCode;

use super::transport::HttpRequestValue;
use crate::schema::{
    AuthKind, HttpContext, HttpRateLimitDetails, HttpRequestDetails, HttpResponseDetails, LlmError,
    LlmErrorReason, ProviderFailureClassification,
};
use crate::shared;
use oc_provider::provider::error::{is_context_overflow, is_quota_exceeded, is_retryable_status};

pub const BODY_LIMIT: usize = 16_384;
pub const MAX_RETRIES: usize = 2;
pub const BASE_DELAY_MS: u64 = 500;
pub const MAX_DELAY_MS: u64 = 10_000;
const REDACTED: &str = "<redacted>";

const SENSITIVE_NAME_SOURCE: &str =
    "authorization|api[-_]?key|access[-_]?token|refresh[-_]?token|id[-_]?token|token|secret|credential|signature|x-amz-signature";

fn sensitive_name_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(&format!("(?i){}", SENSITIVE_NAME_SOURCE)).unwrap())
}

fn sensitive_body_field_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(&format!(
            "(?i)(\"(?:{}|key)\"\\s*:\\s*)\"[^\"]*\"",
            SENSITIVE_NAME_SOURCE
        ))
        .unwrap()
    })
}

fn short_query_name_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"(?i)^(key|sig)$").unwrap())
}

fn is_sensitive_header_name(name: &str) -> bool {
    sensitive_name_regex().is_match(name)
}

fn is_sensitive_query_name(name: &str) -> bool {
    is_sensitive_header_name(name) || short_query_name_regex().is_match(name)
}

fn redact_headers(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (name, value) in headers {
        if is_sensitive_header_name(name) {
            result.insert(name.clone(), REDACTED.to_string());
        } else {
            result.insert(name.clone(), value.clone());
        }
    }
    result
}

fn redact_url(value: &str) -> String {
    let Ok(mut url) = url::Url::parse(value) else {
        return REDACTED.to_string();
    };
    let mut query: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        if is_sensitive_query_name(&key) {
            query.insert(key.to_string(), REDACTED.to_string());
        } else {
            query.insert(key.to_string(), value.to_string());
        }
    }
    url.query_pairs_mut().clear();
    for (key, value) in query {
        url.query_pairs_mut().append_pair(&key, &value);
    }
    url.to_string()
}

fn secret_values(request: &HttpRequestValue) -> Vec<String> {
    let mut values = Vec::new();
    for (name, value) in &request.headers {
        if !is_sensitive_header_name(name) {
            continue;
        }
        if value.len() >= 4 {
            values.push(value.clone());
            values.push(urlencoding(value));
        }
        if let Some(bearer) = value.strip_prefix("Bearer ") {
            if bearer.len() >= 4 {
                values.push(bearer.to_string());
                values.push(urlencoding(bearer));
            }
        }
    }
    values
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn redact_body(body: &str, request: &HttpRequestValue) -> String {
    let mut text = sensitive_body_field_regex()
        .replace_all(body, |caps: &regex::Captures| {
            format!("{}{}", &caps[1], REDACTED)
        })
        .to_string();
    for secret in secret_values(request) {
        text = text.replace(&secret, REDACTED);
    }
    text
}

fn normalized_headers(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect()
}

fn request_id(headers: &BTreeMap<String, String>) -> Option<String> {
    for name in [
        "x-request-id",
        "request-id",
        "x-amzn-requestid",
        "x-amz-request-id",
        "x-goog-request-id",
        "cf-ray",
    ] {
        if let Some(value) = headers.get(name) {
            return Some(value.clone());
        }
    }
    None
}

fn retryable_status(status: StatusCode) -> bool {
    is_retryable_status(status.as_u16())
}

fn retry_after_ms(headers: &BTreeMap<String, String>) -> Option<i64> {
    if let Some(value) = headers.get("retry-after-ms") {
        if let Ok(millis) = value.parse::<f64>() {
            if millis.is_finite() {
                return Some((millis.max(0.0)) as i64);
            }
        }
    }
    let value = headers.get("retry-after")?;
    if let Ok(seconds) = value.parse::<f64>() {
        if seconds.is_finite() {
            return Some((seconds.max(0.0) * 1000.0) as i64);
        }
    }
    // HTTP-date form: approximated conservatively by returning `None`.
    None
}

fn rate_limit_details(
    headers: &BTreeMap<String, String>,
    retry_after: Option<i64>,
) -> Option<HttpRateLimitDetails> {
    let mut limit = BTreeMap::new();
    let mut remaining = BTreeMap::new();
    let mut reset = BTreeMap::new();
    for (name, value) in headers {
        if let Some(key) = name.strip_prefix("x-ratelimit-limit-") {
            limit.insert(key.to_string(), value.clone());
        } else if let Some(key) = name.strip_prefix("x-ratelimit-remaining-") {
            remaining.insert(key.to_string(), value.clone());
        } else if let Some(key) = name.strip_prefix("x-ratelimit-reset-") {
            reset.insert(key.to_string(), value.clone());
        } else if let Some(caps) = anthropic_ratelimit(name) {
            match caps.2.as_str() {
                "limit" => {
                    limit.insert(caps.1, value.clone());
                }
                "remaining" => {
                    remaining.insert(caps.1, value.clone());
                }
                _ => {
                    reset.insert(caps.1, value.clone());
                }
            }
        }
    }
    if retry_after.is_none() && limit.is_empty() && remaining.is_empty() && reset.is_empty() {
        return None;
    }
    Some(HttpRateLimitDetails {
        retry_after_ms: retry_after,
        limit: if limit.is_empty() { None } else { Some(limit) },
        remaining: if remaining.is_empty() {
            None
        } else {
            Some(remaining)
        },
        reset: if reset.is_empty() { None } else { Some(reset) },
    })
}

fn anthropic_ratelimit(name: &str) -> Option<(String, String, String)> {
    let lower = name.to_lowercase();
    let rest = lower.strip_prefix("anthropic-ratelimit-")?.to_string();
    let (kind, rest) = rest.rsplit_once('-')?;
    if !matches!(kind, "limit" | "remaining" | "reset") {
        return None;
    }
    Some((lower, rest.to_string(), kind.to_string()))
}
fn provider_message(status: StatusCode, body: &str) -> String {
    if body.len() <= 500 && !body.is_empty() {
        return format!(
            "Provider request failed with HTTP {}: {}",
            status.as_u16(),
            body
        );
    }
    format!("Provider request failed with HTTP {}", status.as_u16())
}

fn status_reason(
    status: StatusCode,
    message: &str,
    body: &str,
    retry_after_ms: Option<i64>,
    rate_limit: Option<HttpRateLimitDetails>,
    http: HttpContext,
) -> LlmErrorReason {
    let content_policy =
        regex::Regex::new(r"(?i)content[-_\s]?policy|content_filter|safety").unwrap();
    if content_policy.is_match(body) {
        return LlmErrorReason::ContentPolicy {
            message: message.to_string(),
            provider_metadata: None,
            http: Some(http),
        };
    }
    match status.as_u16() {
        401 => LlmErrorReason::Authentication {
            message: message.to_string(),
            kind: AuthKind::Invalid,
            provider_metadata: None,
            http: Some(http),
        },
        403 => LlmErrorReason::Authentication {
            message: message.to_string(),
            kind: AuthKind::InsufficientPermissions,
            provider_metadata: None,
            http: Some(http),
        },
        429 => {
            if is_quota_exceeded(body) {
                LlmErrorReason::QuotaExceeded {
                    message: message.to_string(),
                    provider_metadata: None,
                    http: Some(http),
                }
            } else {
                LlmErrorReason::RateLimit {
                    message: message.to_string(),
                    retry_after_ms,
                    rate_limit,
                    provider_metadata: None,
                    http: Some(http),
                }
            }
        }
        400 | 404 | 409 | 413 | 422 => LlmErrorReason::InvalidRequest {
            message: message.to_string(),
            parameter: None,
            classification: if status == StatusCode::PAYLOAD_TOO_LARGE || is_context_overflow(body)
            {
                Some(ProviderFailureClassification::ContextOverflow)
            } else {
                None
            },
            provider_metadata: None,
            http: Some(http),
        },
        _ if status.as_u16() >= 500 || retryable_status(status) => {
            LlmErrorReason::ProviderInternal {
                message: message.to_string(),
                status: status.as_u16() as i64,
                retry_after_ms,
                provider_metadata: None,
                http: Some(http),
            }
        }
        _ => LlmErrorReason::UnknownProvider {
            message: message.to_string(),
            status: Some(status.as_u16() as i64),
            provider_metadata: None,
            http: Some(http),
        },
    }
}

fn request_details(request: &HttpRequestValue) -> HttpRequestDetails {
    HttpRequestDetails {
        method: "POST".to_string(),
        url: redact_url(&request.url),
        headers: redact_headers(&request.headers),
    }
}

fn response_details(status: StatusCode, headers: &BTreeMap<String, String>) -> HttpResponseDetails {
    HttpResponseDetails {
        status: status.as_u16() as i64,
        headers: redact_headers(headers),
    }
}

fn response_body_text(body: &str, request: &HttpRequestValue) -> (Option<String>, Option<bool>) {
    if body.is_empty() {
        return (None, None);
    }
    let redacted = redact_body(body, request);
    if redacted.len() <= BODY_LIMIT {
        (Some(redacted), None)
    } else {
        (
            Some(redacted.chars().take(BODY_LIMIT).collect()),
            Some(true),
        )
    }
}

/// Build a `LlmError` from a non-2xx response.
/// From reference/packages/llm/src/route/executor.ts (`statusError`)
pub fn status_error(
    request: &HttpRequestValue,
    status: StatusCode,
    body: &str,
    headers: &BTreeMap<String, String>,
) -> LlmError {
    let normalized = normalized_headers(headers);
    let retry_after = retry_after_ms(&normalized);
    let rate_limit = rate_limit_details(&normalized, retry_after);
    let (body_field, body_truncated) = response_body_text(body, request);
    let message = provider_message(status, body_field.as_deref().unwrap_or(""));
    let reason = status_reason(
        status,
        &message,
        body_field.clone().as_deref().unwrap_or(""),
        retry_after,
        rate_limit.clone(),
        HttpContext {
            request: Some(request_details(request)),
            response: Some(response_details(status, headers)),
            body: body_field,
            body_truncated,
            request_id: request_id(&normalized),
            rate_limit,
        },
    );
    LlmError::new("RequestExecutor", "execute", reason)
}

fn transport_error(message: String, kind: Option<String>, url: Option<String>) -> LlmError {
    let http = url.as_ref().map(|url| HttpContext {
        request: Some(HttpRequestDetails {
            method: "POST".to_string(),
            url: redact_url(url),
            headers: BTreeMap::new(),
        }),
        response: None,
        body: None,
        body_truncated: None,
        request_id: None,
        rate_limit: None,
    });
    LlmError::new(
        "RequestExecutor",
        "execute",
        LlmErrorReason::Transport {
            message,
            kind,
            url: url.map(|u| redact_url(&u)),
            http,
        },
    )
}

/// Simple deterministic jitter for retry backoff.
fn retry_delay(error: &LlmError, attempt: usize) -> u64 {
    if let Some(retry_after) = error.retry_after_ms() {
        return (retry_after as u64).min(MAX_DELAY_MS);
    }
    let base = (BASE_DELAY_MS * 2u64.pow(attempt as u32) * 8) / 10;
    (base.min(MAX_DELAY_MS)) + (attempt as u64 * 37) % 20
}

/// `Executor` — one retrying HTTP transport.
/// From reference/packages/llm/src/route/executor.ts (`layer`)
#[derive(Clone)]
pub struct Executor {
    client: reqwest::Client,
}

impl Executor {
    pub fn new(client: reqwest::Client) -> Executor {
        Executor { client }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    async fn execute_once(
        &self,
        request: &HttpRequestValue,
    ) -> Result<reqwest::Response, LlmError> {
        let response = self
            .client
            .post(&request.url)
            .headers(build_headers(&request.headers))
            .body(request.body.clone())
            .send()
            .await
            .map_err(|error| {
                transport_error(
                    format!("HTTP transport failed: {}", error),
                    Some("TransportError".to_string()),
                    Some(request.url.clone()),
                )
            })?;
        if response.status().as_u16() < 400 {
            return Ok(response);
        }
        let status = response.status();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let body = response.text().await.unwrap_or_default();
        Err(status_error(request, status, &body, &headers))
    }

    /// `executor.execute(request)` with retries.
    pub async fn execute(&self, request: &HttpRequestValue) -> Result<reqwest::Response, LlmError> {
        let mut attempt = 0usize;
        loop {
            match self.execute_once(request).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if !error.retryable() || attempt >= MAX_RETRIES {
                        return Err(error);
                    }
                    let delay = retry_delay(&error, attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    attempt += 1;
                }
            }
        }
    }
}

fn build_headers(headers: &BTreeMap<String, String>) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            map.insert(name, value);
        }
    }
    map
}

/// `ProviderShared.errorText` for stream errors.
pub fn stream_error_text(error: &anyhow::Error) -> String {
    error.to_string()
}

#[allow(unused)]
pub(crate) fn _shared_marker() {
    let _ = shared::error_text_str;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> HttpRequestValue {
        let mut headers = BTreeMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer header-secret".to_string(),
        );
        headers.insert("x-safe".to_string(), "visible".to_string());
        HttpRequestValue {
            url: "https://api.example.test/v1?key=url-secret&safe=visible".to_string(),
            body: "{}".to_string(),
            headers,
        }
    }

    #[test]
    fn status_error_marks_retryable_429_and_5xx_but_not_quota() {
        let request = request();
        let headers = BTreeMap::new();
        for status in [429, 500, 501, 502, 503, 504, 529] {
            let body = if status == 429 {
                r#"{"error":{"code":"rate_limit"}}"#
            } else {
                r#"{"error":{"code":"server_error"}}"#
            };
            let error = status_error(
                &request,
                StatusCode::from_u16(status).unwrap(),
                body,
                &headers,
            );
            assert!(error.retryable(), "status {status} should be retryable");
        }

        let quota = status_error(
            &request,
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"code":"insufficient_quota"}}"#,
            &headers,
        );
        assert!(matches!(quota.reason, LlmErrorReason::QuotaExceeded { .. }));
        assert!(!quota.retryable());
    }

    #[test]
    fn status_error_classifies_context_overflow_like_provider_parser() {
        let request = request();
        let headers = BTreeMap::new();
        let by_code = status_error(
            &request,
            StatusCode::BAD_REQUEST,
            r#"{"error":{"code":"context_length_exceeded"}}"#,
            &headers,
        );
        let by_status = status_error(
            &request,
            StatusCode::PAYLOAD_TOO_LARGE,
            "Payload Too Large",
            &headers,
        );
        for error in [by_code, by_status] {
            assert_eq!(
                error.reason.classification(),
                Some(ProviderFailureClassification::ContextOverflow)
            );
            assert!(!error.retryable());
        }
    }

    #[test]
    fn status_error_redacts_request_and_body_secrets() {
        let request = request();
        let headers = BTreeMap::new();
        let error = status_error(
            &request,
            StatusCode::BAD_REQUEST,
            r#"{"apiKey":"body-secret","message":"failed"}"#,
            &headers,
        );
        let http = match &error.reason {
            LlmErrorReason::InvalidRequest {
                http: Some(http), ..
            } => http,
            reason => panic!(
                "expected HTTP invalid-request details, got {}",
                reason.tag()
            ),
        };
        let request_details = http.request.as_ref().unwrap();
        assert!(!request_details.url.contains("url-secret"));
        assert_eq!(request_details.headers["Authorization"], REDACTED);
        assert!(!http.body.as_deref().unwrap().contains("body-secret"));
        assert!(http.body.as_deref().unwrap().contains(REDACTED));
    }
}
