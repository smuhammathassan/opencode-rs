//! `LLMError` and its tagged reasons.
//! From reference/packages/llm/src/schema/errors.ts

use serde_json::Value;
use std::collections::BTreeMap;

use super::events::ProviderFailureClassification;

/// `HttpRequestDetails`.
/// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, Default)]
pub struct HttpRequestDetails {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// `HttpResponseDetails`.
/// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, Default)]
pub struct HttpResponseDetails {
    pub status: i64,
    pub headers: BTreeMap<String, String>,
}

/// `HttpRateLimitDetails`.
/// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, Default)]
pub struct HttpRateLimitDetails {
    pub retry_after_ms: Option<i64>,
    pub limit: Option<BTreeMap<String, String>>,
    pub remaining: Option<BTreeMap<String, String>>,
    pub reset: Option<BTreeMap<String, String>>,
}

/// `HttpContext`.
/// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, Default)]
pub struct HttpContext {
    pub request: Option<HttpRequestDetails>,
    pub response: Option<HttpResponseDetails>,
    pub body: Option<String>,
    pub body_truncated: Option<bool>,
    pub request_id: Option<String>,
    pub rate_limit: Option<HttpRateLimitDetails>,
}

/// `LLMErrorReason` — tagged union of failure reasons.
/// From reference/packages/llm/src/schema/errors.ts (`LLMErrorReason`)
#[derive(Debug, Clone)]
pub enum LlmErrorReason {
    InvalidRequest {
        message: String,
        parameter: Option<String>,
        classification: Option<ProviderFailureClassification>,
        provider_metadata: Option<super::ids::ProviderMetadata>,
        http: Option<HttpContext>,
    },
    NoRoute {
        route: String,
        provider: String,
        model: String,
    },
    Authentication {
        message: String,
        kind: AuthKind,
        provider_metadata: Option<super::ids::ProviderMetadata>,
        http: Option<HttpContext>,
    },
    RateLimit {
        message: String,
        retry_after_ms: Option<i64>,
        rate_limit: Option<HttpRateLimitDetails>,
        provider_metadata: Option<super::ids::ProviderMetadata>,
        http: Option<HttpContext>,
    },
    QuotaExceeded {
        message: String,
        provider_metadata: Option<super::ids::ProviderMetadata>,
        http: Option<HttpContext>,
    },
    ContentPolicy {
        message: String,
        provider_metadata: Option<super::ids::ProviderMetadata>,
        http: Option<HttpContext>,
    },
    ProviderInternal {
        message: String,
        status: i64,
        retry_after_ms: Option<i64>,
        provider_metadata: Option<super::ids::ProviderMetadata>,
        http: Option<HttpContext>,
    },
    Transport {
        message: String,
        kind: Option<String>,
        url: Option<String>,
        http: Option<HttpContext>,
    },
    InvalidProviderOutput {
        message: String,
        route: Option<String>,
        raw: Option<String>,
        provider_metadata: Option<super::ids::ProviderMetadata>,
    },
    UnknownProvider {
        message: String,
        status: Option<i64>,
        provider_metadata: Option<super::ids::ProviderMetadata>,
        http: Option<HttpContext>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    Missing,
    Invalid,
    Expired,
    InsufficientPermissions,
    Unknown,
}

impl AuthKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthKind::Missing => "missing",
            AuthKind::Invalid => "invalid",
            AuthKind::Expired => "expired",
            AuthKind::InsufficientPermissions => "insufficient-permissions",
            AuthKind::Unknown => "unknown",
        }
    }
}

impl LlmErrorReason {
    pub fn tag(&self) -> &'static str {
        match self {
            LlmErrorReason::InvalidRequest { .. } => "InvalidRequest",
            LlmErrorReason::NoRoute { .. } => "NoRoute",
            LlmErrorReason::Authentication { .. } => "Authentication",
            LlmErrorReason::RateLimit { .. } => "RateLimit",
            LlmErrorReason::QuotaExceeded { .. } => "QuotaExceeded",
            LlmErrorReason::ContentPolicy { .. } => "ContentPolicy",
            LlmErrorReason::ProviderInternal { .. } => "ProviderInternal",
            LlmErrorReason::Transport { .. } => "Transport",
            LlmErrorReason::InvalidProviderOutput { .. } => "InvalidProviderOutput",
            LlmErrorReason::UnknownProvider { .. } => "UnknownProvider",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            LlmErrorReason::InvalidRequest { message, .. } => message,
            LlmErrorReason::NoRoute { .. } => "No LLM route",
            LlmErrorReason::Authentication { message, .. } => message,
            LlmErrorReason::RateLimit { message, .. } => message,
            LlmErrorReason::QuotaExceeded { message, .. } => message,
            LlmErrorReason::ContentPolicy { message, .. } => message,
            LlmErrorReason::ProviderInternal { message, .. } => message,
            LlmErrorReason::Transport { message, .. } => message,
            LlmErrorReason::InvalidProviderOutput { message, .. } => message,
            LlmErrorReason::UnknownProvider { message, .. } => message,
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            LlmErrorReason::RateLimit { .. } | LlmErrorReason::ProviderInternal { .. }
        )
    }

    pub fn retry_after_ms(&self) -> Option<i64> {
        match self {
            LlmErrorReason::RateLimit { retry_after_ms, .. }
            | LlmErrorReason::ProviderInternal { retry_after_ms, .. } => *retry_after_ms,
            _ => None,
        }
    }

    pub fn classification(&self) -> Option<ProviderFailureClassification> {
        match self {
            LlmErrorReason::InvalidRequest { classification, .. } => *classification,
            _ => None,
        }
    }
}

/// `LLMError` — module/method/reason error.
/// From reference/packages/llm/src/schema/errors.ts (`LLMError`)
#[derive(Debug, Clone)]
pub struct LlmError {
    pub module: String,
    pub method: String,
    pub reason: LlmErrorReason,
}

impl LlmError {
    pub fn new(
        module: impl Into<String>,
        method: impl Into<String>,
        reason: LlmErrorReason,
    ) -> LlmError {
        LlmError {
            module: module.into(),
            method: method.into(),
            reason,
        }
    }

    pub fn message(&self) -> String {
        match &self.reason {
            LlmErrorReason::NoRoute {
                route,
                provider,
                model,
            } => {
                format!(
                    "{}.{}: No LLM route for {}/{} using {}",
                    self.module, self.method, provider, model, route
                )
            }
            _ => format!("{}.{}: {}", self.module, self.method, self.reason.message()),
        }
    }

    pub fn retryable(&self) -> bool {
        self.reason.retryable()
    }

    pub fn retry_after_ms(&self) -> Option<i64> {
        self.reason.retry_after_ms()
    }

    pub fn is_invalid_request(&self) -> bool {
        matches!(self.reason, LlmErrorReason::InvalidRequest { .. })
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for LlmError {}

/// `InvalidRequestReason` convenience constructors.
/// From reference/packages/llm/src/schema/errors.ts
impl LlmError {
    pub fn invalid_request(message: impl Into<String>) -> LlmError {
        LlmError::new(
            "ProviderShared",
            "request",
            LlmErrorReason::InvalidRequest {
                message: message.into(),
                parameter: None,
                classification: None,
                provider_metadata: None,
                http: None,
            },
        )
    }

    pub fn event_error(route: &str, message: impl Into<String>, raw: Option<String>) -> LlmError {
        LlmError::new(
            "ProviderShared",
            "stream",
            LlmErrorReason::InvalidProviderOutput {
                message: message.into(),
                route: Some(route.to_string()),
                raw,
                provider_metadata: None,
            },
        )
    }
}

/// `ToolFailure` — tool handler failure type.
/// From reference/packages/llm/src/schema/errors.ts (`ToolFailure`)
#[derive(Debug, Clone)]
pub struct ToolFailure {
    pub message: String,
    pub error: Option<Value>,
    pub metadata: Option<BTreeMap<String, Value>>,
}

impl ToolFailure {
    pub fn new(message: impl Into<String>) -> ToolFailure {
        ToolFailure {
            message: message.into(),
            error: None,
            metadata: None,
        }
    }
}

impl std::fmt::Display for ToolFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolFailure {}
