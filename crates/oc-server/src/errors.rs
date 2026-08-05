//! API error contracts.
//!
//! The v2 `/api` surface serializes `Schema.TaggedErrorClass` failures as
//! `{ "_tag": "...", ...fields }` with the status from `httpApiStatus`
//! (reference/packages/protocol/src/errors.ts). The v1 instance surface uses
//! `Schema.ErrorClass` failures shaped `{ "name": "...", "data": {...} }`
//! (reference/packages/opencode/src/server/routes/instance/httpapi/errors.ts).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Map, Value};

/// v2 `/api` error, matching reference/packages/protocol/src/errors.ts.
#[derive(Debug, Clone)]
pub enum ApiError {
    InvalidRequest {
        message: String,
        kind: Option<String>,
        field: Option<String>,
    },
    Unauthorized {
        message: String,
    },
    Conflict {
        message: String,
        resource: Option<String>,
    },
    ServiceUnavailable {
        message: String,
        service: Option<String>,
    },
    Unknown {
        message: String,
        reference: Option<String>,
    },
    ProviderNotFound {
        provider_id: String,
        message: String,
    },
    SessionNotFound {
        session_id: String,
        message: String,
    },
    MessageNotFound {
        session_id: String,
        message_id: String,
        message: String,
    },
    InvalidCursor {
        message: String,
    },
    PermissionNotFound {
        request_id: String,
        message: String,
    },
    QuestionNotFound {
        request_id: String,
        message: String,
    },
    Forbidden {
        message: String,
    },
    PtyNotFound {
        pty_id: String,
        message: String,
    },
    /// Built-in `HttpApiError.BadRequest` (v1 instance surface).
    V1BadRequest,
    /// `ApiNotFoundError` — `{ "name": "NotFoundError", "data": { "message" } }`.
    ApiNotFound {
        message: String,
    },
}

impl ApiError {
    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::InvalidRequest { .. }
            | ApiError::InvalidCursor { .. }
            | ApiError::V1BadRequest => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
            ApiError::Conflict { .. } => StatusCode::CONFLICT,
            ApiError::ServiceUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::Unknown { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::ProviderNotFound { .. }
            | ApiError::SessionNotFound { .. }
            | ApiError::MessageNotFound { .. }
            | ApiError::PermissionNotFound { .. }
            | ApiError::QuestionNotFound { .. }
            | ApiError::PtyNotFound { .. }
            | ApiError::ApiNotFound { .. } => StatusCode::NOT_FOUND,
            ApiError::Forbidden { .. } => StatusCode::FORBIDDEN,
        }
    }

    fn body(&self) -> Value {
        match self {
            ApiError::InvalidRequest {
                message,
                kind,
                field,
            } => {
                let mut map = match message_map(message) {
                    Value::Object(map) => map,
                    _ => Map::new(),
                };
                if let Some(kind) = kind {
                    map.insert("kind".into(), Value::String(kind.clone()));
                }
                if let Some(field) = field {
                    map.insert("field".into(), Value::String(field.clone()));
                }
                tagged("InvalidRequestError", Value::Object(map))
            }
            ApiError::Unauthorized { message } => tagged("UnauthorizedError", message_map(message)),
            ApiError::Conflict { message, resource } => tagged(
                "ConflictError",
                insert_optional(message_map(message), "resource", resource),
            ),
            ApiError::ServiceUnavailable { message, service } => tagged(
                "ServiceUnavailableError",
                insert_optional(message_map(message), "service", service),
            ),
            ApiError::Unknown { message, reference } => tagged(
                "UnknownError",
                insert_optional(message_map(message), "ref", reference),
            ),
            ApiError::ProviderNotFound {
                provider_id,
                message,
            } => tagged(
                "ProviderNotFoundError",
                json!({ "providerID": provider_id, "message": message }),
            ),
            ApiError::SessionNotFound {
                session_id,
                message,
            } => tagged(
                "SessionNotFoundError",
                json!({ "sessionID": session_id, "message": message }),
            ),
            ApiError::MessageNotFound {
                session_id,
                message_id,
                message,
            } => tagged(
                "MessageNotFoundError",
                json!({ "sessionID": session_id, "messageID": message_id, "message": message }),
            ),
            ApiError::InvalidCursor { message } => {
                tagged("InvalidCursorError", message_map(message))
            }
            ApiError::PermissionNotFound {
                request_id,
                message,
            } => tagged(
                "PermissionNotFoundError",
                json!({ "requestID": request_id, "message": message }),
            ),
            ApiError::QuestionNotFound {
                request_id,
                message,
            } => tagged(
                "QuestionNotFoundError",
                json!({ "requestID": request_id, "message": message }),
            ),
            ApiError::Forbidden { message } => tagged("ForbiddenError", message_map(message)),
            ApiError::PtyNotFound { pty_id, message } => tagged(
                "PtyNotFoundError",
                json!({ "ptyID": pty_id, "message": message }),
            ),
            ApiError::V1BadRequest => json!({
                "name": "BadRequest",
                "data": { "message": "Bad request" },
            }),
            ApiError::ApiNotFound { message } => json!({
                "name": "NotFoundError",
                "data": { "message": message },
            }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut response = (status, axum::Json(self.body())).into_response();
        response
            .headers_mut()
            .insert("content-type", "application/json".parse().unwrap());
        response
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        tracing::error!(?error, "json serialization failure");
        ApiError::Unknown {
            message: "Unexpected server error. Check server logs for details.".into(),
            reference: None,
        }
    }
}

fn tagged(name: &str, fields: Value) -> Value {
    match fields {
        Value::Object(mut map) => {
            map.insert("_tag".into(), Value::String(name.into()));
            Value::Object(map)
        }
        other => other,
    }
}

fn message_map(message: &str) -> Value {
    let mut map = Map::new();
    map.insert("message".into(), Value::String(message.into()));
    Value::Object(map)
}

fn insert_optional(fields: Value, key: &str, value: &Option<String>) -> Value {
    match fields {
        Value::Object(mut map) => {
            if let Some(value) = value {
                map.insert(key.into(), Value::String(value.clone()));
            }
            Value::Object(map)
        }
        other => other,
    }
}

/// v2 `notFound` helper for message requests (reference/packages/server/src/handlers/message.ts).
pub fn message_not_found(session_id: &str, message_id: &str) -> ApiError {
    ApiError::MessageNotFound {
        session_id: session_id.to_string(),
        message_id: message_id.to_string(),
        message: format!("Message not found: {message_id}"),
    }
}

pub fn session_not_found(session_id: &str) -> ApiError {
    ApiError::SessionNotFound {
        session_id: session_id.to_string(),
        message: format!("Session not found: {session_id}"),
    }
}
