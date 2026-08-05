//! Client and protocol error types.
//!
//! `ClientError` mirrors `reference/packages/client/src/generated/client-error.ts`.
//! `ProtocolError` mirrors `reference/packages/protocol/src/errors.ts`.
//! The declared error bodies are decoded from responses whose status is in the
//! endpoint's `declaredStatuses` (see `reference/packages/client/src/generated/client.ts`).

use serde_json::Value;

/// Transport-level client errors. Mirrors `ClientErrorReason` in
/// `reference/packages/client/src/generated/client-error.ts`.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The request could not be performed (network failure, timeout, ...).
    #[error("transport error")]
    Transport(#[source] reqwest::Error),
    /// The response status was not the endpoint's success status nor one of its declared statuses.
    #[error("unexpected status: {0}")]
    UnexpectedStatus(u16),
    /// The response content-type was not `application/json`/`+json` or `text/event-stream`.
    #[error("unsupported content type")]
    UnsupportedContentType,
    /// The response body was empty, too large, or not valid JSON.
    #[error("malformed response")]
    MalformedResponse(#[source] Option<serde_json::Error>),
}

/// Errors declared by the server API. Mirrors `reference/packages/protocol/src/errors.ts`
/// (the `_tag`-discriminated error classes).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "_tag")]
pub enum ProtocolError {
    InvalidRequestError {
        message: String,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        field: Option<String>,
    },
    UnauthorizedError {
        message: String,
    },
    ConflictError {
        message: String,
        #[serde(default)]
        resource: Option<String>,
    },
    ServiceUnavailableError {
        message: String,
        #[serde(default)]
        service: Option<String>,
    },
    UnknownError {
        message: String,
        #[serde(rename = "ref", default)]
        reference: Option<String>,
    },
    ProviderNotFoundError {
        #[serde(rename = "providerID")]
        provider_id: String,
        message: String,
    },
    SessionNotFoundError {
        #[serde(rename = "sessionID")]
        session_id: String,
        message: String,
    },
    MessageNotFoundError {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        message: String,
    },
    InvalidCursorError {
        message: String,
    },
    PermissionNotFoundError {
        #[serde(rename = "requestID")]
        request_id: String,
        message: String,
    },
    QuestionNotFoundError {
        #[serde(rename = "requestID")]
        request_id: String,
        message: String,
    },
    ForbiddenError {
        message: String,
    },
    PtyNotFoundError {
        #[serde(rename = "ptyID")]
        pty_id: String,
        message: String,
    },
}

/// `ProjectCopyError` is `name`-tagged instead of `_tag`-tagged.
/// From reference/packages/protocol/src/groups/project-copy.ts.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectCopyError {
    pub message: String,
    pub force_required: Option<bool>,
}

/// A decoded server error that could not be matched to a known `_tag`.
/// Carries the raw body so callers can inspect unknown error payloads.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownApiError(pub Value);

/// A server-declared error. The server returns one of these with one of the
/// endpoint's declared statuses; the client decodes and surfaces it directly,
/// mirroring `throw await json(response)` in
/// `reference/packages/client/src/generated/client.ts`.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    Protocol(ProtocolError),
    ProjectCopy(ProjectCopyError),
    Unknown(UnknownApiError),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Protocol(protocol) => write!(formatter, "{protocol:?}"),
            ApiError::ProjectCopy(project_copy) => write!(formatter, "{project_copy:?}"),
            ApiError::Unknown(unknown) => write!(formatter, "{unknown:?}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProtocolError {}

impl std::fmt::Display for ProjectCopyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ProjectCopyError {}

impl std::fmt::Display for UnknownApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for UnknownApiError {}

/// The unified error type returned by client methods.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Api(#[from] ApiError),
}

impl From<serde_json::Error> for ClientError {
    fn from(err: serde_json::Error) -> Self {
        ClientError::MalformedResponse(Some(err))
    }
}

impl Error {
    /// Type guard mirroring `isUnauthorizedError` in
    /// `reference/packages/client/src/generated/types.ts`.
    pub fn is_unauthorized(&self) -> bool {
        matches!(
            self,
            Error::Api(ApiError::Protocol(ProtocolError::UnauthorizedError { .. }))
        )
    }

    /// Type guard mirroring `isInvalidRequestError`.
    pub fn is_invalid_request(&self) -> bool {
        matches!(
            self,
            Error::Api(ApiError::Protocol(
                ProtocolError::InvalidRequestError { .. }
            ))
        )
    }

    /// Type guard mirroring `isInvalidCursorError`.
    pub fn is_invalid_cursor(&self) -> bool {
        matches!(
            self,
            Error::Api(ApiError::Protocol(ProtocolError::InvalidCursorError { .. }))
        )
    }

    /// Type guard mirroring `isSessionNotFoundError`.
    pub fn is_session_not_found(&self) -> bool {
        matches!(
            self,
            Error::Api(ApiError::Protocol(
                ProtocolError::SessionNotFoundError { .. }
            ))
        )
    }

    /// Type guard mirroring `isMessageNotFoundError`.
    pub fn is_message_not_found(&self) -> bool {
        matches!(
            self,
            Error::Api(ApiError::Protocol(
                ProtocolError::MessageNotFoundError { .. }
            ))
        )
    }

    /// Type guard mirroring `isConflictError`.
    pub fn is_conflict(&self) -> bool {
        matches!(
            self,
            Error::Api(ApiError::Protocol(ProtocolError::ConflictError { .. }))
        )
    }

    /// Type guard mirroring `isServiceUnavailableError`.
    pub fn is_service_unavailable(&self) -> bool {
        matches!(
            self,
            Error::Api(ApiError::Protocol(
                ProtocolError::ServiceUnavailableError { .. }
            ))
        )
    }
}

/// Decode a server error body into an `ApiError`.
///
/// Declared protocol errors use an `_tag` discriminator; `ProjectCopyError`
/// (400) uses a `name` discriminator. Anything else is surfaced raw.
pub(crate) fn decode_api_error(value: Value) -> ApiError {
    if let Some(name) = value.get("name").and_then(Value::as_str) {
        if name == "ProjectCopyError" {
            return ApiError::ProjectCopy(ProjectCopyError {
                message: value
                    .pointer("/data/message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                force_required: value
                    .pointer("/data/forceRequired")
                    .and_then(Value::as_bool),
            });
        }
    }
    match serde_json::from_value::<ProtocolError>(value.clone()) {
        Ok(protocol) => ApiError::Protocol(protocol),
        Err(_) => ApiError::Unknown(UnknownApiError(value)),
    }
}
