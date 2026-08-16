//! v2 session handler. From reference/packages/server/src/handlers/session.ts.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use base64::Engine;
use serde_json::{json, Value};

use super::{json, json_value, no_content, request_location, HandlerResult};
use crate::errors::{session_not_found, ApiError};
use crate::event::{session_id, session_message_id};
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

/// Clone the selected history into a new session without reusing globally
/// unique message ids. The SQLite `message.id` key is shared across sessions,
/// so persisting a source id under the child would move that row out of the
/// parent session.
fn fork_messages(
    source_id: &str,
    messages: &[Value],
    message_id: Option<&str>,
    child_id: &str,
) -> Result<Vec<Value>, ApiError> {
    let end = match message_id {
        Some(message_id) => messages
            .iter()
            .position(|message| message.get("id").and_then(Value::as_str) == Some(message_id))
            .map(|index| index + 1)
            .ok_or_else(|| ApiError::MessageNotFound {
                session_id: source_id.to_string(),
                message_id: message_id.to_string(),
                message: format!("Message not found: {message_id}"),
            })?,
        None => messages.len(),
    };

    Ok(messages[..end]
        .iter()
        .map(|message| {
            let mut forked = message.clone();
            if let Some(object) = forked.as_object_mut() {
                if object.contains_key("id") {
                    object.insert("id".into(), Value::String(session_message_id()));
                }
                if object.contains_key("sessionID") {
                    object.insert("sessionID".into(), Value::String(child_id.to_string()));
                }
            }
            forked
        })
        .collect())
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
    state.persist_session(&info);
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

/// POST /api/session/:sessionID/fork.
///
/// Fork the selected in-memory history and persist the child projection. The
/// copied messages receive fresh ids because the durable message table keys by
/// id rather than by `(session_id, id)`.
pub async fn session_fork(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<Value>,
) -> HandlerResult {
    let source_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let requested_message_id = body.get("messageID").and_then(Value::as_str);

    let source = {
        let stores = state.stores.read().await;
        stores
            .sessions
            .get(&source_id)
            .cloned()
            .ok_or_else(|| session_not_found(&source_id))?
    };

    let child_id = session_id();
    let messages = fork_messages(
        &source_id,
        &source.messages,
        requested_message_id,
        &child_id,
    )?;
    let created = timestamp();
    let mut info = source.info;
    info.id = child_id.clone();
    info.parent_id = Some(source_id);
    info.time.created = created;
    info.time.updated = created;
    info.title = oc_session::session::get_forked_title(&info.title);

    let record = SessionRecord {
        info: info.clone(),
        messages: messages.clone(),
        active: false,
    };
    let mut stores = state.stores.write().await;
    stores.sessions.insert(child_id, record);
    drop(stores);

    state.persist_session(&info);
    for message in &messages {
        state.persist_message(&info.id, message);
    }

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
    let info = record.info.clone();
    drop(stores);
    state.persist_session(&info);
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
    let info = record.info.clone();
    drop(stores);
    state.persist_session(&info);
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
        .filter(|delivery| matches!(*delivery, "steer" | "queue"))
        .unwrap_or("steer")
        .to_string();
    let resume = body.get("resume").and_then(|v| v.as_bool());

    let mut stores = state.stores.write().await;
    if !stores.sessions.contains_key(&session_id) {
        return Err(session_not_found(&session_id));
    }
    let record = stores.sessions.get_mut(&session_id).unwrap();
    let created = timestamp();
    let mut message = message::user(
        &id,
        created,
        prompt.get("text").and_then(|t| t.as_str()).unwrap_or(""),
    );
    if let Some(files) = body
        .get("files")
        .or_else(|| prompt.get("files"))
        .and_then(Value::as_array)
    {
        message["files"] = Value::Array(files.clone());
    }
    if let Some(agents) = body
        .get("agents")
        .or_else(|| prompt.get("agents"))
        .and_then(Value::as_array)
    {
        message["agents"] = Value::Array(agents.clone());
    }
    if let Some(metadata) = body.get("metadata").or_else(|| prompt.get("metadata")) {
        message["metadata"] = metadata.clone();
    }
    record.messages.push(message.clone());
    record.info.time.updated = created;
    record.active = true;
    let info = record.info.clone();
    let admitted_seq = record.messages.len() as i64;
    drop(stores);
    state.persist_session(&info);
    state.persist_message(&session_id, &message);
    state
        .enqueue_session_input(
            &session_id,
            id.clone(),
            prompt.clone(),
            admitted_seq as u64,
            delivery.clone(),
        )
        .await;
    let prompt_event = json!({
        "timestamp": created,
        "sessionID": session_id,
        "messageID": id,
        "prompt": prompt,
        "delivery": delivery,
    });
    for event_type in ["session.next.prompted", "session.next.prompt.admitted"] {
        state.emit_event(crate::event::Event {
            id: crate::event::event_id(),
            metadata: None,
            r#type: event_type.into(),
            durable: None,
            location: None,
            data: prompt_event.clone(),
        });
    }
    if delivery == "steer" {
        state.interrupt_session_for_steer(&session_id).await;
    }

    if let Some(model) = body.get("model") {
        let mut stores = state.stores.write().await;
        if let Some(record) = stores.sessions.get_mut(&session_id) {
            record.info.model = Some(model_from_value(model));
            record.info.time.updated = created;
            let info = record.info.clone();
            drop(stores);
            state.persist_session(&info);
        }
    }
    crate::runner::schedule_session_run(state.clone(), session_id.clone());
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
    drop(stores);
    let _ = crate::runner::compact_session(
        &state,
        &session_id,
        oc_session_runner::session::message::CompactionReason::Manual,
    )
    .await;
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
    {
        let stores = state.stores.read().await;
        if !stores.sessions.contains_key(&session_id) {
            return Err(session_not_found(&session_id));
        }
    }
    let wait = async {
        loop {
            let active = state
                .stores
                .read()
                .await
                .sessions
                .get(&session_id)
                .map(|record| record.active)
                .unwrap_or(false);
            if !active {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(300), wait).await;
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
    let mut revert = serde_json::Map::new();
    revert.insert("messageID".into(), Value::String(message_id));
    if let Some(snapshot) = body.get("snapshot").and_then(Value::as_str) {
        revert.insert("snapshot".into(), Value::String(snapshot.to_string()));
    }
    let revert = Value::Object(revert);
    record.info.revert = Some(revert.clone());
    let persisted_info = record.info.clone();
    drop(stores);
    state.persist_session(&persisted_info);
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
    let persisted_info = record.info.clone();
    drop(stores);
    state.persist_session(&persisted_info);
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
    let snapshot = {
        let stores = state.stores.read().await;
        stores
            .sessions
            .get(&session_id)
            .ok_or_else(|| session_not_found(&session_id))?
            .info
            .revert
            .as_ref()
            .and_then(|revert| revert.get("snapshot"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    if let Some(snapshot) = snapshot {
        crate::instance_handlers::restore_project_snapshot(&state, &snapshot).await?;
    }
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    record.info.revert = None;
    let persisted_info = record.info.clone();
    drop(stores);
    state.persist_session(&persisted_info);
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

    {
        let stores = state.stores.read().await;
        if !stores.sessions.contains_key(&session_id) {
            return Err(session_not_found(&session_id));
        }
    }
    let manifest = oc_sync::sync::store::session_durable_definitions()
        .into_iter()
        .map(|definition| definition.storage_type())
        .collect::<Vec<_>>();
    let (events, has_more) = state
        .sync_store
        .read_aggregate(&session_id, after, limit, &manifest)
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to read session history: {error}"),
            reference: None,
        })?;
    let data = events
        .into_iter()
        .filter_map(|event| serde_json::to_value(event).ok())
        .collect();
    json(&crate::schema::SessionHistory { data, has_more })
}

pub async fn session_events(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
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
    let after = query
        .get("after")
        .and_then(|value| value.parse::<i64>().ok());
    Ok(crate::sse::session_event_stream(state, session_id, after))
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
    state.cancel_session_run(&session_id).await;
    state.emit_event(crate::event::Event {
        id: crate::event::event_id(),
        metadata: None,
        r#type: "session.status".into(),
        durable: None,
        location: None,
        data: json!({
            "sessionID": session_id,
            "status": { "type": "idle" },
        }),
    });
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
