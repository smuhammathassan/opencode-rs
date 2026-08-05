//! Project-copy handler. From reference/packages/server/src/handlers/project-copy.ts.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{json, no_content, request_location, HandlerResult};
use crate::errors::ApiError;
use std::collections::HashMap;

/// `ProjectCopy.create(...)` from `reference/packages/server/src/handlers/project-copy.ts`.
/// TODO(integration): oc-project copy service.
pub async fn project_copy_create(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let project_id = params
        .get("projectID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("copy")
        .to_string();
    json(&serde_json::json!({
        "projectID": project_id,
        "directory": format!("{}-copy", location.directory),
        "name": name,
        "sourceDirectory": location.directory,
    }))
}

pub async fn project_copy_remove(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    no_content()
}

pub async fn project_copy_refresh(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    no_content()
}
