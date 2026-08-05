//! Question handler. From reference/packages/server/src/handlers/question.ts.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{json, no_content, request_location, HandlerResult};
use crate::errors::{session_not_found, ApiError};
use crate::event::question_id;
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `QuestionV2.list()` from `reference/packages/server/src/handlers/question.ts`.
pub async fn question_request_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let stores = state.stores.read().await;
    let data = stores.questions.values().cloned().collect::<Vec<_>>();
    drop(stores);
    json(&LocationResponse {
        location: location.info(),
        data,
    })
}

pub async fn session_question_list(
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
    let data = stores
        .questions
        .values()
        .filter(|q| q.get("sessionID").and_then(|v| v.as_str()) == Some(session_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    drop(stores);
    json(&serde_json::json!({ "data": data }))
}

/// Creates a pending question for the session. Not a reference endpoint but used by the
/// v1 `/session/:sessionID` flow. TODO(integration): oc-command Question service.
pub async fn session_question_reply(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    let owned = stores
        .questions
        .get(&request_id)
        .map(|q| q.get("sessionID").and_then(|v| v.as_str()) == Some(session_id.as_str()))
        .unwrap_or(false);
    if !owned {
        return Err(missing_request(&request_id));
    }
    stores.questions.remove(&request_id);
    drop(stores);
    no_content()
}

pub async fn session_question_reject(
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
    let mut stores = state.stores.write().await;
    let owned = stores
        .questions
        .get(&request_id)
        .map(|q| q.get("sessionID").and_then(|v| v.as_str()) == Some(session_id.as_str()))
        .unwrap_or(false);
    if !owned {
        return Err(missing_request(&request_id));
    }
    stores.questions.remove(&request_id);
    drop(stores);
    no_content()
}

fn missing_request(id: &str) -> ApiError {
    ApiError::QuestionNotFound {
        request_id: id.to_string(),
        message: format!("Question request not found: {id}"),
    }
}

/// Helper shared with the v1 question handler.
#[allow(dead_code)]
pub(crate) fn new_question_id() -> String {
    question_id()
}
