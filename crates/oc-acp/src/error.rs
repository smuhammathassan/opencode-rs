//! ACP service errors.
//!
//! From reference/packages/opencode/src/acp/error.ts. Errors are mapped to ACP
//! `RequestError` objects (JSON-RPC error responses) by [`to_request_error`].

use serde_json::Value;

use crate::types::RequestError;

/// Typed errors surfaced by the ACP service.
#[derive(Debug, Clone, PartialEq)]
pub enum ACPError {
    /// `ACPSessionNotFoundError`
    SessionNotFound { session_id: String },
    /// `ACPInvalidConfigOptionError`
    InvalidConfigOption { config_id: String },
    /// `ACPInvalidModelError`
    InvalidModel {
        model_id: String,
        provider_id: Option<String>,
    },
    /// `ACPInvalidEffortError`
    InvalidEffort { effort: String },
    /// `ACPInvalidModeError`
    InvalidMode { mode: String },
    /// `ACPAuthRequiredError`
    AuthRequired { provider_id: Option<String> },
    /// `ACPUnknownAuthMethodError`
    UnknownAuthMethod { method_id: String },
    /// `ACPUnsupportedOperationError`
    UnsupportedOperation { method: String },
    /// `ACPServiceFailureError`
    ServiceFailure {
        safe_message: String,
        service: Option<String>,
        error_name: Option<String>,
    },
}

impl std::fmt::Display for ACPError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ACPError::SessionNotFound { session_id } => {
                write!(f, "session not found: {session_id}")
            }
            ACPError::InvalidConfigOption { config_id } => {
                write!(f, "unknown config option: {config_id}")
            }
            ACPError::InvalidModel { model_id, .. } => write!(f, "model not found: {model_id}"),
            ACPError::InvalidEffort { effort } => write!(f, "effort not found: {effort}"),
            ACPError::InvalidMode { mode } => write!(f, "mode not found: {mode}"),
            ACPError::AuthRequired { .. } => write!(f, "provider authentication required"),
            ACPError::UnknownAuthMethod { method_id } => {
                write!(f, "unknown auth method: {method_id}")
            }
            ACPError::UnsupportedOperation { .. } => write!(f, "method not found"),
            ACPError::ServiceFailure { safe_message, .. } => write!(f, "{safe_message}"),
        }
    }
}

impl std::error::Error for ACPError {}

/// `toRequestError` from reference/packages/opencode/src/acp/error.ts.
pub fn to_request_error(error: &ACPError) -> RequestError {
    match error {
        ACPError::SessionNotFound { session_id } => RequestError::invalid_params(
            Some(serde_json::json!({ "sessionId": session_id })),
            Some(&format!("session not found: {session_id}")),
        ),
        ACPError::InvalidConfigOption { config_id } => RequestError::invalid_params(
            Some(serde_json::json!({ "configId": config_id })),
            Some(&format!("unknown config option: {config_id}")),
        ),
        ACPError::InvalidModel {
            provider_id,
            model_id,
        } => {
            let mut data = serde_json::Map::new();
            if let Some(provider_id) = provider_id {
                data.insert("providerId".into(), Value::String(provider_id.clone()));
            }
            data.insert("modelId".into(), Value::String(model_id.clone()));
            RequestError::invalid_params(
                Some(Value::Object(data)),
                Some(&format!("model not found: {model_id}")),
            )
        }
        ACPError::InvalidEffort { effort } => RequestError::invalid_params(
            Some(serde_json::json!({ "effort": effort })),
            Some(&format!("effort not found: {effort}")),
        ),
        ACPError::InvalidMode { mode } => RequestError::invalid_params(
            Some(serde_json::json!({ "mode": mode })),
            Some(&format!("mode not found: {mode}")),
        ),
        ACPError::AuthRequired { provider_id } => {
            let data = Some(match provider_id {
                Some(provider_id) => serde_json::json!({ "providerId": provider_id }),
                None => serde_json::json!({}),
            });
            RequestError::auth_required(data, Some("provider authentication required"))
        }
        ACPError::UnknownAuthMethod { method_id } => RequestError::invalid_params(
            Some(serde_json::json!({ "methodId": method_id })),
            Some(&format!("unknown auth method: {method_id}")),
        ),
        ACPError::UnsupportedOperation { method } => RequestError::method_not_found(method),
        ACPError::ServiceFailure {
            safe_message,
            service,
            error_name,
        } => {
            let mut data = serde_json::Map::new();
            if let Some(service) = service {
                data.insert("service".into(), Value::String(service.clone()));
            }
            if let Some(error_name) = error_name {
                data.insert("errorName".into(), Value::String(error_name.clone()));
            }
            RequestError::internal_error(Some(Value::Object(data)), Some(safe_message))
        }
    }
}

/// `fromUnknownDefect` from reference/packages/opencode/src/acp/error.ts.
pub fn from_unknown_defect(safe_message: &str) -> ACPError {
    ACPError::ServiceFailure {
        safe_message: safe_message.to_string(),
        service: None,
        error_name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_not_found_error() {
        let error = to_request_error(&ACPError::SessionNotFound {
            session_id: "s1".into(),
        });
        assert_eq!(error.code, -32602);
        assert_eq!(error.message, "Invalid params: session not found: s1");
        assert_eq!(error.data, Some(serde_json::json!({ "sessionId": "s1" })));
    }

    #[test]
    fn auth_required_without_provider() {
        let error = to_request_error(&ACPError::AuthRequired { provider_id: None });
        assert_eq!(error.code, -32000);
        assert_eq!(
            error.message,
            "Authentication required: provider authentication required"
        );
        assert_eq!(error.data, Some(serde_json::json!({})));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": -32000,
                "message": "Authentication required: provider authentication required",
                "data": {}
            })
        );
    }

    #[test]
    fn method_not_found_error() {
        let error = to_request_error(&ACPError::UnsupportedOperation {
            method: "session/load".into(),
        });
        assert_eq!(error.code, -32601);
        assert_eq!(error.message, "\"Method not found\": session/load");
        assert_eq!(
            error.data,
            Some(serde_json::json!({ "method": "session/load" }))
        );
    }

    #[test]
    fn internal_error_with_service() {
        let error = to_request_error(&ACPError::ServiceFailure {
            safe_message: "boom".into(),
            service: Some("session".into()),
            error_name: Some("ApiError".into()),
        });
        assert_eq!(error.code, -32603);
        assert_eq!(error.message, "Internal error: boom");
        assert_eq!(
            error.data,
            Some(serde_json::json!({ "service": "session", "errorName": "ApiError" }))
        );
    }
}
