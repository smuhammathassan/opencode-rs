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
    body: axum::extract::Json<serde_json::Value>,
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
    let owned = stores
        .questions
        .get(&request_id)
        .map(|q| q.get("sessionID").and_then(|v| v.as_str()) == Some(session_id.as_str()))
        .unwrap_or(false);
    if !owned {
        return Err(missing_request(&request_id));
    }
    let answers_value = body
        .get("answers")
        .cloned()
        .unwrap_or_else(|| body.0.clone());
    let answers: Vec<oc_command::question::Answer> = serde_json::from_value(answers_value)
        .map_err(|error| ApiError::Unknown {
            message: format!("invalid question answers: {error}"),
            reference: None,
        })?;
    drop(stores);
    state
        .question_service
        .reply(
            &oc_command::question::QuestionId::new(request_id.clone()),
            answers,
        )
        .map_err(|error| missing_request_with_error(&request_id, error.to_string()))?;
    state.stores.write().await.questions.remove(&request_id);
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
    let stores = state.stores.read().await;
    let owned = stores
        .questions
        .get(&request_id)
        .map(|q| q.get("sessionID").and_then(|v| v.as_str()) == Some(session_id.as_str()))
        .unwrap_or(false);
    if !owned {
        return Err(missing_request(&request_id));
    }
    drop(stores);
    state
        .question_service
        .reject(&oc_command::question::QuestionId::new(request_id.clone()))
        .map_err(|error| missing_request_with_error(&request_id, error.to_string()))?;
    state.stores.write().await.questions.remove(&request_id);
    no_content()
}

fn missing_request(id: &str) -> ApiError {
    ApiError::QuestionNotFound {
        request_id: id.to_string(),
        message: format!("Question request not found: {id}"),
    }
}

fn missing_request_with_error(id: &str, message: String) -> ApiError {
    ApiError::QuestionNotFound {
        request_id: id.to_string(),
        message,
    }
}

/// Helper shared with the v1 question handler.
#[allow(dead_code)]
pub(crate) fn new_question_id() -> String {
    question_id()
}
