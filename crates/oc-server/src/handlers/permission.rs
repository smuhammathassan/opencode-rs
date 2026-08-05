//! Permission handler. From reference/packages/server/src/handlers/permission.ts.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{json, no_content, request_location, HandlerResult};
use crate::errors::{session_not_found, ApiError};
use crate::event::permission_id;
use crate::schema::{
    LocationResponse, PermissionCreateData, PermissionEffect, PermissionSavedData,
};
use crate::state::timestamp;
use std::collections::HashMap;

/// `PermissionV2.list()` from `reference/packages/server/src/handlers/permission.ts`.
pub async fn permission_request_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let stores = state.stores.read().await;
    let data = stores.permissions.values().cloned().collect::<Vec<_>>();
    drop(stores);
    json(&LocationResponse {
        location: location.info(),
        data,
    })
}

/// `permission.ask(...)` from `reference/packages/server/src/handlers/permission.ts`.
/// TODO(integration): evaluate against oc-core permission service.
pub async fn session_permission_create(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    if !stores.sessions.contains_key(&session_id) {
        return Err(session_not_found(&session_id));
    }
    drop(stores);

    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(permission_id);
    let request = body.0.clone();
    let mut stores = state.stores.write().await;
    stores.permissions.insert(id.clone(), request);
    drop(stores);
    json(&PermissionCreateData {
        data: PermissionEffect {
            id,
            effect: "allow".into(),
        },
    })
}

pub async fn session_permission_list(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    if !stores.sessions.contains_key(&session_id) {
        return Err(session_not_found(&session_id));
    }
    let data = stores.permissions.values().cloned().collect::<Vec<_>>();
    drop(stores);
    json(&serde_json::json!({ "data": data }))
}

pub async fn session_permission_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let request = stores.permissions.get(&request_id).cloned();
    drop(stores);
    let Some(request) = request else {
        return Err(missing_request(&request_id));
    };
    let _ = session_id;
    json(&serde_json::json!({ "data": request }))
}

pub async fn session_permission_reply(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let reply = body
        .get("reply")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let _ = reply;
    let _ = timestamp();
    let mut stores = state.stores.write().await;
    if stores.permissions.remove(&request_id).is_none() {
        return Err(missing_request(&request_id));
    }
    drop(stores);
    no_content()
}

pub async fn permission_saved_list(
    State(_state): State<crate::state::AppState>,
    Query(_query): Query<HashMap<String, String>>,
) -> HandlerResult {
    json(&PermissionSavedData { data: Vec::new() })
}

pub async fn permission_saved_remove(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    no_content()
}

fn missing_request(id: &str) -> ApiError {
    ApiError::PermissionNotFound {
        request_id: id.to_string(),
        message: format!("Permission request not found: {id}"),
    }
}
