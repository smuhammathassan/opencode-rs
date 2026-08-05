use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::{ProviderErrorEvent, ProviderFailureClassification};

/// `LLM.Error.InvalidRequest` reason.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidRequestReason {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<ProviderFailureClassification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<crate::llm::ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpContext>,
}

impl InvalidRequestReason {
    pub fn retryable(&self) -> bool {
        false
    }
}

/// `LLM.Error.Authentication` reason.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationReason {
    pub message: String,
    pub kind: AuthenticationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<crate::llm::ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthenticationKind {
    Missing,
    Invalid,
    Expired,
    #[serde(rename = "insufficient-permissions")]
    InsufficientPermissions,
    Unknown,
}

/// `LLM.Error.RateLimit` reason.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitReason {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<crate::llm::ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpContext>,
}

/// `LLM.Error.ProviderInternal` reason.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInternalReason {
    pub message: String,
    pub status: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<crate::llm::ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpContext>,
}

/// Generic non-retryable reason carriers (NoRoute, QuotaExceeded, ContentPolicy,
/// Transport, InvalidProviderOutput, UnknownProvider) that only carry a message.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonMessage {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<crate::llm::ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpContext>,
}

/// `LLM.Error` reason union.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "_tag", rename_all = "kebab-case")]
pub enum LLMErrorReason {
    #[serde(rename = "InvalidRequest")]
    InvalidRequest(InvalidRequestReason),
    #[serde(rename = "NoRoute")]
    NoRoute(ReasonMessage),
    #[serde(rename = "Authentication")]
    Authentication(AuthenticationReason),
    #[serde(rename = "RateLimit")]
    RateLimit(RateLimitReason),
    #[serde(rename = "QuotaExceeded")]
    QuotaExceeded(ReasonMessage),
    #[serde(rename = "ContentPolicy")]
    ContentPolicy(ReasonMessage),
    #[serde(rename = "ProviderInternal")]
    ProviderInternal(ProviderInternalReason),
    #[serde(rename = "Transport")]
    Transport(ReasonMessage),
    #[serde(rename = "InvalidProviderOutput")]
    InvalidProviderOutput(ReasonMessage),
    #[serde(rename = "UnknownProvider")]
    UnknownProvider(ReasonMessage),
}

impl LLMErrorReason {
    pub fn message(&self) -> &str {
        match self {
            Self::InvalidRequest(reason) => &reason.message,
            Self::NoRoute(reason) => &reason.message,
            Self::Authentication(reason) => &reason.message,
            Self::RateLimit(reason) => &reason.message,
            Self::QuotaExceeded(reason) => &reason.message,
            Self::ContentPolicy(reason) => &reason.message,
            Self::ProviderInternal(reason) => &reason.message,
            Self::Transport(reason) => &reason.message,
            Self::InvalidProviderOutput(reason) => &reason.message,
            Self::UnknownProvider(reason) => &reason.message,
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::RateLimit(_) | Self::ProviderInternal(_))
    }

    pub fn retry_after_ms(&self) -> Option<f64> {
        match self {
            Self::RateLimit(reason) => reason.retry_after_ms,
            Self::ProviderInternal(reason) => reason.retry_after_ms,
            _ => None,
        }
    }
}

/// `LLM.HttpContext`
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpContext {
    pub request: HttpRequestDetails,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<HttpResponseDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<HttpRateLimitDetails>,
}

/// `LLM.HttpRequestDetails`
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestDetails {
    pub method: String,
    pub url: String,
    pub headers: serde_json::Map<String, Value>,
}

/// `LLM.HttpResponseDetails`
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponseDetails {
    pub status: f64,
    pub headers: serde_json::Map<String, Value>,
}

/// `LLM.HttpRateLimitDetails`
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRateLimitDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset: Option<serde_json::Map<String, Value>>,
}

/// `LLM.Error` — the typed stream/generate failure.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LLMError {
    pub module: String,
    pub method: String,
    pub reason: Box<LLMErrorReason>,
}

impl std::fmt::Display for LLMError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}: {}",
            self.module,
            self.method,
            self.reason.message()
        )
    }
}

impl std::error::Error for LLMError {}

impl LLMError {
    pub fn retryable(&self) -> bool {
        self.reason.retryable()
    }

    pub fn retry_after_ms(&self) -> Option<f64> {
        self.reason.retry_after_ms()
    }
}

/// `LLM.ToolFailure` — recoverable tool-handler failure.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFailure {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

/// Matches `isContextOverflow` from provider-error.ts. The pattern set mirrors
/// the reference exactly; each entry is anchored case-insensitively.
/// /// From reference/packages/llm/src/provider-error.ts
pub fn is_context_overflow(message: &str) -> bool {
    let exclusions = [
        r"^(throttling error|service unavailable):",
        r"rate limit",
        r"too many requests",
    ];
    if exclusions.iter().any(|pattern| {
        regex::Regex::new(&format!("(?i){pattern}"))
            .expect("static exclusion pattern")
            .is_match(message)
    }) {
        return false;
    }
    let patterns = [
        r"prompt is too long",
        r"request_too_large",
        r"input is too long for requested model",
        r"exceeds the context window",
        r"exceeds (?:the )?(?:model'?s )?maximum context length(?: of [\d,]+ tokens?|\s*\([\d,]+\))",
        r"input token count.*exceeds the maximum",
        r"tokens in request more than max tokens allowed",
        r"maximum prompt length is \d+",
        r"reduce the length of the messages",
        r"maximum context length is \d+ tokens",
        r"exceeds (?:the )?maximum allowed input length of [\d,]+ tokens?",
        r"input \(\d+ tokens\) is longer than the model'?s context length \(\d+ tokens\)",
        r"exceeds the limit of \d+",
        r"exceeds the available context size",
        r"greater than the context length",
        r"context window exceeds limit",
        r"exceeded model token limit",
        r"context[_ ]length[_ ]exceeded",
        r"request entity too large",
        r"context length is only \d+ tokens",
        r"input length.*exceeds.*context length",
        r"prompt too long; exceeded (?:max )?context length",
        r"too large for model with \d+ maximum context length",
        r"prompt has [\d,]+ tokens?, but the configured context size is [\d,]+ tokens?",
        r"model_context_window_exceeded",
        r"too many tokens",
        r"token limit exceeded",
    ];
    if patterns.iter().any(|pattern| {
        regex::Regex::new(&format!("(?i){pattern}"))
            .expect("static pattern")
            .is_match(message)
    }) {
        return true;
    }
    regex::Regex::new(r"(?i)^4(00|13)\s*(status code)?\s*\(no body\)")
        .expect("static pattern")
        .is_match(message)
}

/// Matches `isContextOverflowFailure`: true for an `InvalidRequest` LLM error
/// classified `context-overflow`, or a `provider-error` event so classified.
/// /// From reference/packages/llm/src/provider-error.ts
pub fn is_context_overflow_failure(failure: &RunFailure) -> bool {
    match failure {
        RunFailure::Error(error) => matches!(
            error.reason.as_ref(),
            LLMErrorReason::InvalidRequest(reason)
                if reason.classification == Some(ProviderFailureClassification::ContextOverflow)
        ),
        RunFailure::Event(event) => {
            event.classification == Some(ProviderFailureClassification::ContextOverflow)
        }
    }
}

/// A provider turn can fail either with a typed `LLMError` or an inline
/// `provider-error` event captured from the stream.
/// /// From reference/packages/llm/src/provider-error.ts
#[derive(Debug, Clone, PartialEq)]
pub enum RunFailure {
    Error(LLMError),
    Event(ProviderErrorEvent),
}
