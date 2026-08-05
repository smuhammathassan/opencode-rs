//! v2 session handler. From reference/packages/server/src/handlers/session.ts.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use base64::Engine;
use serde_json::{json, Value};

use super::{json, json_value, no_content, request_location, HandlerResult};
use crate::errors::{session_not_found, ApiError};
use crate::event::{event_id, session_id, session_message_id};
use crate::schema::{message, Admitted, SessionCursor, SessionInfo, SessionsResponse};
use crate::state::{timestamp, SessionRecord};

const DEFAULT_SESSIONS_LIMIT: usize = 50;
const DEFAULT_SESSION_HISTORY_LIMIT: usize = 50;
const BASE64_URL: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Session list cursor. From reference/packages/protocol/src/groups/session.ts
/// (`SessionsCursor`): `base64url(JSON.stringify({...query, anchor}))`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SessionsCursorPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subpath: Option<String>,
    anchor: Anchor,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Anchor {
    id: String,
    time: i64,
    direction: String,
}

fn encode_sessions_cursor(query: &SessionQuery, anchor: Anchor) -> String {
    let payload = SessionsCursorPayload {
        workspace: query.workspace.clone(),
        order: query.order.clone(),
        search: query.search.clone(),
        directory: query.directory.clone(),
        project: query.project.clone(),
        subpath: query.subpath.clone(),
        anchor,
    };
    let json = serde_json::to_string(&payload).unwrap_or_default();
    BASE64_URL.encode(json.as_bytes())
}

fn decode_sessions_cursor(input: &str) -> Result<SessionsCursorPayload, ApiError> {
    let decoded = BASE64_URL
        .decode(input.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| ApiError::InvalidCursor {
            message: "Invalid cursor".into(),
        })?;
    serde_json::from_str(&decoded).map_err(|_| ApiError::InvalidCursor {
        message: "Invalid cursor".into(),
    })
}

/// Parsed `/api/session` query. From reference/packages/protocol/src/groups/session.ts
/// (`SessionsQuery`).
#[derive(Debug, Default, Clone)]
pub struct SessionQuery {
    pub workspace: Option<String>,
    pub limit: Option<usize>,
    pub order: Option<String>,
    pub search: Option<String>,
    pub directory: Option<String>,
    pub project: Option<String>,
    pub subpath: Option<String>,
    pub cursor: Option<String>,
}

impl SessionQuery {
    pub fn from_map(map: &HashMap<String, String>) -> SessionQuery {
        SessionQuery {
            workspace: map.get("workspace").cloned(),
            limit: map.get("limit").and_then(|v| v.parse().ok()),
            order: map.get("order").cloned(),
            search: map.get("search").cloned(),
            directory: map.get("directory").cloned(),
            project: map.get("project").cloned(),
            subpath: map.get("subpath").cloned(),
            cursor: map.get("cursor").cloned(),
        }
    }
}

pub async fn session_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let mut query = SessionQuery::from_map(&params);
    let _location = request_location(&state, params.get("location").map(|_| ""), &headers);

    let stores = state.stores.read().await;
    let sessions = stores
        .sessions
        .values()
        .map(|s| s.info.clone())
        .collect::<Vec<_>>();
    drop(stores);

    let (order, cursor_filter) = if let Some(cursor) = query.cursor.clone() {
        let decoded = decode_sessions_cursor(&cursor)?;
        query.order = decoded.order.clone();
        query.search = decoded.search.clone();
        query.directory = decoded.directory.clone();
        query.project = decoded.project.clone();
        query.subpath = decoded.subpath.clone();
        query.workspace = decoded.workspace.clone();
        (
            decoded.order.clone().unwrap_or_else(|| "desc".into()),
            Some(decoded.anchor),
        )
    } else {
        (query.order.clone().unwrap_or_else(|| "desc".into()), None)
    };

    let mut filtered: Vec<SessionInfo> = sessions
        .into_iter()
        .filter(|s| {
            let in_location = query
                .directory
                .as_ref()
                .map_or(true, |d| s.location.directory == *d);
            let in_project = query.project.as_ref().map_or(true, |p| s.project_id == *p);
            let matches_search = query
                .search
                .as_ref()
                .map_or(true, |q| s.title.contains(q) || s.id.contains(q));
            in_location && in_project && matches_search
        })
        .collect();

    filtered.sort_by(|a, b| {
        if order == "asc" {
            a.time.created.cmp(&b.time.created)
        } else {
            b.time.created.cmp(&a.time.created)
        }
    });

    if let Some(anchor) = cursor_filter {
        let keep = match anchor.direction.as_str() {
            "next" => filtered
                .iter()
                .position(|s| s.id == anchor.id)
                .and_then(|i| i.checked_add(1)),
            _ => filtered
                .iter()
                .position(|s| s.id == anchor.id)
                .map(|i| i.saturating_sub(1)),
        };
        match keep {
            Some(i) => filtered = filtered.into_iter().skip(i).collect(),
            None => filtered.clear(),
        }
    }

    let limit = query.limit.unwrap_or(DEFAULT_SESSIONS_LIMIT);
    let page = filtered.into_iter().take(limit).collect::<Vec<_>>();
    let first = page.first();
    let last = page.last();

    let cursor = SessionCursor {
        previous: first.map(|s| {
            encode_sessions_cursor(
                &query,
                Anchor {
                    id: s.id.clone(),
                    time: s.time.created,
                    direction: "previous".into(),
                },
            )
        }),
        next: last.map(|s| {
            encode_sessions_cursor(
                &query,
                Anchor {
                    id: s.id.clone(),
                    time: s.time.created,
                    direction: "next".into(),
                },
            )
        }),
    };

    json(&SessionsResponse { data: page, cursor })
}

fn info_from_record(record: &SessionRecord) -> SessionInfo {
    record.info.clone()
}

pub async fn session_create(
    State(state): State<crate::state::AppState>,
    headers: HeaderMap,
    body: axum::extract::Json<Value>,
) -> HandlerResult {
    let location = request_location(&state, None, &headers);
    let directory = body
        .get("location")
        .and_then(|l| l.get("directory"))
        .and_then(|d| d.as_str())
        .map(|d| d.to_string())
        .unwrap_or_else(|| location.directory.clone());

    let created = timestamp();
    let info = SessionInfo {
        id: body
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(session_id),
        parent_id: None,
        project_id: location.project_id.clone(),
        agent: body
            .get("agent")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        model: body.get("model").map(model_from_value),
        cost: 0.0,
        tokens: crate::schema::Tokens {
            input: 0.0,
            output: 0.0,
            reasoning: 0.0,
            cache: crate::schema::CacheTokens {
                read: 0.0,
                write: 0.0,
            },
        },
        time: crate::schema::SessionTime {
            created,
            updated: created,
            archived: None,
        },
        title: "New Session".into(),
        location: crate::schema::LocationRef {
            directory,
            workspace_id: location.workspace_id.clone(),
        },
        subpath: None,
        revert: None,
    };
    let record = SessionRecord {
        info: info.clone(),
        messages: Vec::new(),
        active: false,
    };

    let mut stores = state.stores.write().await;
    stores.sessions.insert(info.id.clone(), record);
    drop(stores);
    json(&crate::schema::SessionData { data: info })
}

fn model_from_value(value: &Value) -> crate::schema::ModelRef {
    crate::schema::ModelRef {
        id: value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        provider_id: value
            .get("providerID")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        variant: value
            .get("variant")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

pub async fn session_active(State(state): State<crate::state::AppState>) -> HandlerResult {
    let stores = state.stores.read().await;
    let mut data = serde_json::Map::new();
    for (id, record) in &stores.sessions {
        if record.active {
            data.insert(id.clone(), json!({ "type": "running" }));
        }
    }
    drop(stores);
    json(&crate::schema::SessionsActive { data })
}

fn get_session(stores: &crate::state::Stores, session_id: &str) -> Result<SessionInfo, ApiError> {
    stores
        .sessions
        .get(session_id)
        .map(info_from_record)
        .ok_or_else(|| session_not_found(session_id))
}

pub async fn session_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let info = get_session(&stores, &session_id)?;
    drop(stores);
    json(&crate::schema::SessionData { data: info })
}

pub async fn session_switch_agent(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let agent = body
        .get("agent")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    record.info.agent = Some(agent);
    record.info.time.updated = timestamp();
    drop(stores);
    no_content()
}

pub async fn session_switch_model(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let model = model_from_value(body.get("model").unwrap_or(&Value::Null));
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    record.info.model = Some(model);
    record.info.time.updated = timestamp();
    drop(stores);
    no_content()
}

pub async fn session_prompt(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let prompt = body
        .get("prompt")
        .cloned()
        .unwrap_or_else(|| json!({ "text": "" }));
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(session_message_id);
    let delivery = body
        .get("delivery")
        .and_then(|v| v.as_str())
        .unwrap_or("steer")
        .to_string();
    let resume = body.get("resume").and_then(|v| v.as_bool());

    let mut stores = state.stores.write().await;
    if !stores.sessions.contains_key(&session_id) {
        return Err(session_not_found(&session_id));
    }
    let record = stores.sessions.get_mut(&session_id).unwrap();
    let created = timestamp();
    record.messages.push(message::user(
        &id,
        created,
        prompt.get("text").and_then(|t| t.as_str()).unwrap_or(""),
    ));
    record.info.time.updated = created;
    record.active = true;
    let admitted_seq = record.messages.len() as i64;
    drop(stores);

    let text = prompt
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let _ = text;
    let _ = resume;
    let admitted = Admitted {
        admitted_seq,
        id,
        session_id,
        prompt,
        delivery,
        time_created: created,
        promoted_seq: None,
    };
    json_value(serde_json::json!({ "data": admitted }))
}

pub async fn session_compact(
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
    no_content()
}

pub async fn session_wait(
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
    no_content()
}

pub async fn session_revert_stage(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let message_id = body
        .get("messageID")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    let has_message = record
        .messages
        .iter()
        .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(message_id.as_str()));
    if !has_message {
        let not_found = ApiError::MessageNotFound {
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            message: format!("Message not found: {message_id}"),
        };
        return Err(not_found);
    }
    let revert = json!({
        "messageID": message_id,
    });
    record.info.revert = Some(revert.clone());
    drop(stores);
    json(&json!({ "data": revert }))
}

pub async fn session_revert_clear(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    record.info.revert = None;
    drop(stores);
    no_content()
}

pub async fn session_revert_commit(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    record.info.revert = None;
    drop(stores);
    no_content()
}

pub async fn session_context(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let record = stores
        .sessions
        .get(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    let data = record.messages.clone();
    drop(stores);
    json(&crate::schema::ContextData { data })
}

pub async fn session_history(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let after = query.get("after").and_then(|v| v.parse::<i64>().ok());
    let limit = query
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SESSION_HISTORY_LIMIT);

    let stores = state.stores.read().await;
    let record = stores
        .sessions
        .get(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    let data: Vec<Value> = record
        .messages
        .iter()
        .enumerate()
        .filter(|(i, _)| after.map_or(true, |after| (*i as i64) > after))
        .map(|(i, m)| {
            json!({
                "id": event_id(),
                "type": "session.next.prompted",
                "data": {
                    "sessionID": session_id,
                    "messageID": m.get("id").cloned().unwrap_or_default(),
                    "prompt": m,
                    "delivery": "steer",
                    "timestamp": timestamp(),
                },
                "durable": {
                    "aggregateID": session_id,
                    "seq": i,
                    "version": 1,
                },
            })
        })
        .take(limit)
        .collect();
    let has_more = data.len() == limit;
    drop(stores);
    json(&crate::schema::SessionHistory { data, has_more })
}

pub async fn session_events(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    // Mirrors `session.events` (SSE). The router-level handler wraps this as a stream;
    // this entrypoint just validates the session exists.
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    {
        let stores = state.stores.read().await;
        if !stores.sessions.contains_key(&session_id) {
            return Err(session_not_found(&session_id));
        }
    }
    Ok(crate::sse::session_event_stream(state))
}

pub async fn session_interrupt(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    if let Some(record) = stores.sessions.get_mut(&session_id) {
        record.active = false;
    } else {
        return Err(session_not_found(&session_id));
    }
    drop(stores);
    no_content()
}

pub async fn session_message(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let message_id = params
        .get("messageID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let record = stores
        .sessions
        .get(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    let message = record
        .messages
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(message_id.as_str()))
        .cloned()
        .ok_or_else(|| ApiError::MessageNotFound {
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            message: format!("Message not found: {message_id}"),
        })?;
    drop(stores);
    json(&json!({ "data": message }))
}
