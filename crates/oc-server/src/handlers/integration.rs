//! Integration handler. From reference/packages/server/src/handlers/integration.ts.
//!
//! Integration discovery and OAuth are wired to oc-core's `Integration` service in the
//! reference; this crate returns empty catalog data until that integration lands.
//! TODO(integration): delegate to oc-core Integration service.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{json, no_content, request_location, HandlerResult};
use crate::errors::ApiError;
use crate::schema::LocationResponse;
use std::collections::HashMap;

pub async fn integration_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    json(&LocationResponse {
        location: location.info(),
        data: Vec::<serde_json::Value>::new(),
    })
}

pub async fn integration_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let _integration_id = params
        .get("integrationID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    json(&LocationResponse {
        location: location.info(),
        data: serde_json::Value::Null,
    })
}

pub async fn integration_connect_key(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    no_content()
}

pub async fn integration_connect_oauth(
    State(state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    json(&LocationResponse {
        location: location.info(),
        data: serde_json::json!({
            "id": crate::event::event_id(),
            "status": "pending",
        }),
    })
}

pub async fn integration_attempt_status(
    State(state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    json(&LocationResponse {
        location: location.info(),
        data: serde_json::json!({ "status": "failed" }),
    })
}

pub async fn integration_attempt_complete(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    no_content()
}

pub async fn integration_attempt_cancel(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    no_content()
}
