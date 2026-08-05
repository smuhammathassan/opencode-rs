//! v2 `/api` handlers. Each module mirrors one file under
//! reference/packages/server/src/handlers/.

pub mod agent;
pub mod command;
pub mod credential;
pub mod event;
pub mod fs;
pub mod health;
pub mod integration;
pub mod location;
pub mod message;
pub mod model;
pub mod permission;
pub mod project_copy;
pub mod provider;
pub mod pty;
pub mod question;
pub mod reference;
pub mod session;
pub mod skill;

use std::collections::HashMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::errors::ApiError;
use crate::location::Location;
use crate::state::AppState;

pub type HandlerResult = Result<Response, ApiError>;

/// Serialize a handler value as `application/json`.
pub fn json<T: serde::Serialize>(value: &T) -> HandlerResult {
    Ok(axum::Json(value).into_response())
}

/// Serialize an already-built JSON value.
pub fn json_value(value: serde_json::Value) -> HandlerResult {
    Ok(axum::Json(value).into_response())
}

/// HTTP 204 (`HttpApiSchema.NoContent`).
pub fn no_content() -> HandlerResult {
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Resolve the request location. From reference/packages/server/src/location.ts (`ref`).
pub fn request_location(state: &AppState, query: Option<&str>, headers: &HeaderMap) -> Location {
    crate::location::resolve_location(query, headers, &state.location)
}

/// Parse the raw query string into a flat map.
pub fn query_map(query: Option<&str>) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.unwrap_or("").as_bytes())
        .into_owned()
        .collect()
}

/// True when a request is a WebSocket/SSE connection that should stay open.
pub fn is_sse_accept(header_value: Option<&axum::http::HeaderValue>) -> bool {
    header_value
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false)
}
