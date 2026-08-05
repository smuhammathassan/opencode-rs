//! v1 instance handlers.
//!
//! Port of the legacy instance surface from
//! reference/packages/opencode/src/server/routes/instance/httpapi/handlers/*. Many
//! routes depend on oc-core services that are not integrated yet and return stable
//! empty/default shapes. TODO(integration): wire each group to its oc-* service.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::errors::ApiError;
use crate::handlers::{json_value, no_content, HandlerResult};
use crate::schema::{LocationRef, ModelRef, SessionInfo};
use crate::state::{now_millis, AppState};

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

/// GET /config. From reference/packages/opencode/src/server/routes/instance/httpapi/
/// handlers/config.ts (`config.get`). TODO(integration): oc-config merged view.
pub async fn config_get(State(state): State<crate::state::AppState>) -> HandlerResult {
    let stores = state.stores.read().await;
    let config = stores.config.clone();
    drop(stores);
    json_value(config)
}

/// PATCH /config. From reference/.../handlers/config.ts (`config.update`).
pub async fn config_update(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let mut stores = state.stores.write().await;
    stores.config = body.0;
    let config = stores.config.clone();
    drop(stores);
    json_value(config)
}

/// GET /config/providers. From reference/.../handlers/config.ts (`config.providers`).
pub async fn config_providers(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({ "providers": [], "default": {} }))
}

/// GET /global/config. From reference/.../groups/global.ts.
pub async fn global_config_get(State(state): State<crate::state::AppState>) -> HandlerResult {
    let stores = state.stores.read().await;
    let config = stores.config.clone();
    drop(stores);
    json_value(config)
}

/// PATCH /global/config.
pub async fn global_config_update(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let mut stores = state.stores.write().await;
    stores.config = body.0;
    let config = stores.config.clone();
    drop(stores);
    json_value(config)
}

// ---------------------------------------------------------------------------
// global
// ---------------------------------------------------------------------------

/// GET /global/health. From reference/.../groups/global.ts: `{ healthy: true, version }`.
pub async fn global_health(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({ "healthy": true, "version": crate::version() }))
}

/// GET /global/event (SSE). From reference/.../handlers/event.ts.
pub async fn global_event(State(state): State<crate::state::AppState>) -> HandlerResult {
    Ok(crate::sse::v1_event_stream(state.events))
}

/// POST /global/dispose. From reference/.../groups/global.ts.
pub async fn global_dispose(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /global/upgrade.
pub async fn global_upgrade(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::json!({ "success": false, "error": "upgrade is not implemented" }))
}

// ---------------------------------------------------------------------------
// instance
// ---------------------------------------------------------------------------

/// POST /instance/dispose. From reference/.../groups/instance.ts.
pub async fn instance_dispose(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// GET /path. From reference/.../groups/instance.ts (`PathInfo`).
pub async fn instance_path(State(state): State<crate::state::AppState>) -> HandlerResult {
    let directory = state.location.directory.clone();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let state_dir =
        std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| format!("{home}/.local/state/opencode"));
    let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    json_value(serde_json::json!({
        "home": home,
        "state": state_dir,
        "config": config_dir,
        "worktree": directory,
        "directory": directory,
    }))
}

/// GET /vcs. TODO(integration): oc-core git.
pub async fn vcs_get(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({
        "command": "git",
        "state": { "mode": "no-git" },
    }))
}

/// GET /vcs/status.
pub async fn vcs_status(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /vcs/diff.
pub async fn vcs_diff(
    State(_state): State<crate::state::AppState>,
    _query: Query<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /vcs/diff/raw.
pub async fn vcs_diff_raw(
    State(_state): State<crate::state::AppState>,
    _query: Query<HashMap<String, String>>,
) -> HandlerResult {
    Ok((
        [("content-type", "text/x-diff; charset=utf-8")],
        String::new(),
    )
        .into_response())
}

/// POST /vcs/apply. TODO(integration): oc-core patch apply.
pub async fn vcs_apply(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::json!({ "success": false }))
}

/// GET /command. From reference/.../groups/instance.ts (`command.list`).
pub async fn command_list(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /agent. From reference/.../groups/instance.ts (`app.agents`).
pub async fn agent_list(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /skill. From reference/.../groups/instance.ts (`app.skills`).
pub async fn skill_list(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /lsp. From reference/.../groups/instance.ts (`lsp.status`).
pub async fn lsp_status(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /formatter.
pub async fn formatter_status(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

// ---------------------------------------------------------------------------
// v1 session
// ---------------------------------------------------------------------------

fn v1_info(info: &SessionInfo, _directory: &str) -> serde_json::Value {
    serde_json::json!({
        "id": info.id,
        "slug": info.id.split('_').last().unwrap_or(&info.id),
        "projectID": info.project_id,
        "directory": info.location.directory,
        "cost": info.cost,
        "tokens": {
            "input": info.tokens.input,
            "output": info.tokens.output,
            "reasoning": info.tokens.reasoning,
            "cache": { "read": info.tokens.cache.read, "write": info.tokens.cache.write },
        },
        "title": info.title,
        "agent": info.agent,
        "model": info.model.as_ref().map(|m| serde_json::json!({ "id": m.id, "providerID": m.provider_id, "variant": m.variant })),
        "version": crate::version(),
        "time": {
            "created": info.time.created,
            "updated": info.time.updated,
            "archived": info.time.archived,
        },
    })
}

/// GET /session. From reference/.../handlers/session.ts (`session.list`).
pub async fn session_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let stores = state.stores.read().await;
    let mut sessions = stores
        .sessions
        .values()
        .map(|r| v1_info(&r.info, &state.location.directory))
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| {
        let at = a["time"]["updated"].as_i64().unwrap_or(0);
        let bt = b["time"]["updated"].as_i64().unwrap_or(0);
        bt.cmp(&at)
    });
    drop(stores);
    json_value(serde_json::Value::Array(sessions))
}

/// GET /session/status.
pub async fn session_status(State(state): State<crate::state::AppState>) -> HandlerResult {
    let stores = state.stores.read().await;
    let mut map = serde_json::Map::new();
    for (id, record) in &stores.sessions {
        map.insert(
            id.clone(),
            serde_json::json!({
                "status": if record.active { "active" } else { "idle" },
            }),
        );
    }
    drop(stores);
    json_value(serde_json::Value::Object(map))
}

/// GET /session/:sessionID.
pub async fn session_get(
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
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    let info = v1_info(&record.info, &state.location.directory);
    drop(stores);
    json_value(info)
}

/// GET /session/:sessionID/message. Returns v1 messages (`WithParts`).
pub async fn session_messages(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let record = stores
        .sessions
        .get(&session_id)
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    let mut messages = record.messages.clone();
    if query.get("limit").is_some() {
        if let Some(limit) = query.get("limit").and_then(|v| v.parse::<usize>().ok()) {
            messages.truncate(limit);
        }
    }
    drop(stores);
    json_value(serde_json::Value::Array(messages))
}

/// GET /session/:sessionID/message/:messageID.
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
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    let message = record
        .messages
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(message_id.as_str()))
        .cloned();
    drop(stores);
    json_value(message.ok_or_else(|| ApiError::ApiNotFound {
        message: "Message not found".into(),
    })?)
}

/// POST /session. From reference/.../handlers/session.ts (`session.create`).
pub async fn session_create(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let created = now_millis();
    let id = crate::event::session_id();
    let model = body
        .get("model")
        .and_then(|m| m.as_object())
        .map(|m| ModelRef {
            id: m
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            provider_id: m
                .get("providerID")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            variant: m
                .get("variant")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });
    let info = SessionInfo {
        id: id.clone(),
        parent_id: body
            .get("parentID")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        project_id: state.location.project_id.clone(),
        agent: body
            .get("agent")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        model,
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
        title: body
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("New Session")
            .to_string(),
        location: LocationRef {
            directory: state.location.directory.clone(),
            workspace_id: state.location.workspace_id.clone(),
        },
        subpath: None,
        revert: None,
    };
    let mut stores = state.stores.write().await;
    stores.sessions.insert(
        id,
        crate::state::SessionRecord {
            info: info.clone(),
            messages: Vec::new(),
            active: false,
        },
    );
    drop(stores);
    json_value(v1_info(&info, &state.location.directory))
}

/// PATCH /session/:sessionID.
pub async fn session_update(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    if let Some(title) = body.get("title").and_then(|v| v.as_str()) {
        record.info.title = title.to_string();
    }
    let info = v1_info(&record.info, &state.location.directory);
    drop(stores);
    json_value(info)
}

/// DELETE /session/:sessionID.
pub async fn session_delete(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    if stores.sessions.remove(&session_id).is_none() {
        return Err(ApiError::ApiNotFound {
            message: "Session not found".into(),
        });
    }
    drop(stores);
    json_value(serde_json::Value::Bool(true))
}

/// POST /session/:sessionID/abort.
pub async fn session_abort(
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
    }
    drop(stores);
    json_value(serde_json::Value::Bool(true))
}

/// POST /session/:sessionID/message. From reference/.../handlers/session.ts
/// (`session.prompt`). Minimal WithParts shape; TODO(integration): oc-session runner.
pub async fn session_prompt(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let created = now_millis();
    let message_id = crate::event::session_message_id();
    let text = body
        .get("prompt")
        .or_else(|| body.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let message = serde_json::json!({
        "id": message_id,
        "time": { "created": created },
        "type": "user",
        "text": text,
        "files": [],
        "agents": [],
    });
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    record.messages.push(message.clone());
    record.active = true;
    drop(stores);
    json_value(serde_json::json!({
        "info": {
            "id": message_id,
            "role": "user",
            "time": { "created": created },
        },
        "parts": [message],
    }))
}

/// POST /session/:sessionID/prompt_async. Returns 204.
pub async fn session_prompt_async(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let _ = session_prompt(State(state), Path(params), body).await?;
    no_content()
}

/// POST /session/:sessionID/command.
pub async fn session_command(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    session_prompt(State(state), Path(params), body).await
}

/// POST /session/:sessionID/shell.
pub async fn session_shell(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    session_prompt(State(state), Path(params), body).await
}

/// POST /session/:sessionID/revert. TODO(integration): oc-session revert.
pub async fn session_revert(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let info = stores
        .sessions
        .get(&session_id)
        .map(|r| v1_info(&r.info, &state.location.directory))
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    drop(stores);
    json_value(info)
}

/// POST /session/:sessionID/unrevert.
pub async fn session_unrevert(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    session_revert(
        State(state),
        Path(params),
        axum::extract::Json(serde_json::Value::Null),
    )
    .await
}

/// GET /session/:sessionID/children.
pub async fn session_children(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /session/:sessionID/todo.
pub async fn session_todo(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /session/:sessionID/diff.
pub async fn session_diff(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _query: Query<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// POST /session/:sessionID/fork.
pub async fn session_fork(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    session_get(State(state), Path(params)).await
}

/// POST /session/:sessionID/share.
pub async fn session_share(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    Err(ApiError::Unknown {
        message: "Sharing is not configured".into(),
        reference: None,
    })
}

/// DELETE /session/:sessionID/share.
pub async fn session_unshare(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    Err(ApiError::Unknown {
        message: "Sharing is not configured".into(),
        reference: None,
    })
}

/// POST /session/:sessionID/init.
pub async fn session_init(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /session/:sessionID/summarize.
pub async fn session_summarize(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /session/:sessionID/permissions/:permissionID. Deprecated in the reference.
pub async fn session_permission_respond(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// DELETE /session/:sessionID/message/:messageID.
pub async fn session_delete_message(
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
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::ApiNotFound {
            message: "Session not found".into(),
        })?;
    let before = record.messages.len();
    record
        .messages
        .retain(|m| m.get("id").and_then(|v| v.as_str()) != Some(message_id.as_str()));
    if record.messages.len() == before {
        return Err(ApiError::ApiNotFound {
            message: "Message not found".into(),
        });
    }
    drop(stores);
    json_value(serde_json::Value::Bool(true))
}

/// DELETE /session/:sessionID/message/:messageID/part/:partID.
pub async fn session_delete_part(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// PATCH /session/:sessionID/message/:messageID/part/:partID.
pub async fn session_update_part(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(body.0)
}

// ---------------------------------------------------------------------------
// event / pty / question / permission (v1)
// ---------------------------------------------------------------------------

/// GET /event (SSE). From reference/.../handlers/event.ts.
pub async fn event_subscribe(State(state): State<crate::state::AppState>) -> HandlerResult {
    Ok(crate::sse::v1_event_stream(state.events))
}

/// GET /pty/shells.
pub async fn pty_shells(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!([
        { "path": "/bin/sh", "name": "sh", "acceptable": true },
        { "path": "/bin/bash", "name": "bash", "acceptable": true },
    ]))
}

/// GET /question.
pub async fn question_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let stores = state.stores.read().await;
    let data = stores.questions.values().cloned().collect::<Vec<_>>();
    drop(stores);
    json_value(serde_json::Value::Array(data))
}

/// POST /question/:requestID/reply.
pub async fn question_reply(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    if stores.questions.remove(&request_id).is_none() {
        return Err(ApiError::QuestionNotFound {
            request_id,
            message: "Question request not found".into(),
        });
    }
    drop(stores);
    json_value(serde_json::Value::Bool(true))
}

/// POST /question/:requestID/reject.
pub async fn question_reject(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    question_reply(
        State(state),
        Path(params),
        axum::extract::Json(serde_json::Value::Null),
    )
    .await
}

/// GET /permission.
pub async fn permission_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let stores = state.stores.read().await;
    let data = stores.permissions.values().cloned().collect::<Vec<_>>();
    drop(stores);
    json_value(serde_json::Value::Array(data))
}

/// POST /permission/:requestID/reply.
pub async fn permission_reply(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    if stores.permissions.remove(&request_id).is_none() {
        return Err(ApiError::PermissionNotFound {
            request_id,
            message: "Permission request not found".into(),
        });
    }
    drop(stores);
    json_value(serde_json::Value::Bool(true))
}

// ---------------------------------------------------------------------------
// project / provider / file / mcp / sync / tui / experimental / workspace / control
// ---------------------------------------------------------------------------

/// GET /project. TODO(integration): oc-project.
pub async fn project_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!([project_info(&state)]))
}

/// GET /project/current.
pub async fn project_current(State(state): State<crate::state::AppState>) -> HandlerResult {
    json_value(project_info(&state))
}

/// POST /project/git/init.
pub async fn project_git_init(State(state): State<crate::state::AppState>) -> HandlerResult {
    json_value(project_info(&state))
}

fn project_info(state: &AppState) -> serde_json::Value {
    serde_json::json!({
        "id": state.location.project_id,
        "name": None::<String>,
        "worktree": state.location.directory,
        "directory": state.location.directory,
        "path": null,
        "icon": null,
        "commands": [],
        "agent": null,
        "version": crate::version(),
        "share": null,
        "archived": false,
    })
}

/// PATCH /project/:projectID.
pub async fn project_update(
    State(state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(project_info(&state))
}

/// GET /project/:projectID/directories.
pub async fn project_directories(
    State(state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::json!({ "directories": [state.location.directory] }))
}

/// GET /provider. TODO(integration): oc-provider.
pub async fn provider_list(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({
        "providers": [],
        "default": null,
        "authenticated": {},
        "preferred": [],
    }))
}

/// GET /provider/auth.
pub async fn provider_auth(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// POST /provider/:providerID/oauth/authorize.
pub async fn provider_oauth_authorize(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Null)
}

/// POST /provider/:providerID/oauth/callback.
pub async fn provider_oauth_callback(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(false))
}

/// GET /find. TODO(integration): oc-util ripgrep.
pub async fn find_text(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let _pattern = query.get("pattern").cloned().unwrap_or_default();
    let _ = &state;
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /find/file.
pub async fn find_file(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let _query_text = query.get("query").cloned().unwrap_or_default();
    let _ = &state;
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /find/symbol.
pub async fn find_symbol(
    State(_state): State<crate::state::AppState>,
    _query: Query<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /file. TODO(integration): oc-core filesystem.
pub async fn file_list(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let directory = query
        .get("directory")
        .cloned()
        .unwrap_or_else(|| state.location.directory.clone());
    let path = query.get("path").cloned().unwrap_or_default();
    let dir = std::path::PathBuf::from(directory).join(path);
    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(&dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let absolute = entry.path().to_string_lossy().into_owned();
            entries.push(serde_json::json!({
                "name": name,
                "path": name,
                "absolute": absolute,
                "type": if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { "directory" } else { "file" },
                "ignored": false,
            }));
        }
    }
    json_value(serde_json::Value::Array(entries))
}

/// GET /file/content. TODO(integration): oc-core filesystem.
pub async fn file_content(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let directory = query
        .get("directory")
        .cloned()
        .unwrap_or_else(|| state.location.directory.clone());
    let path = query.get("path").cloned().unwrap_or_default();
    let file = std::path::PathBuf::from(directory).join(path);
    let content = std::fs::read(&file).unwrap_or_default();
    json_value(serde_json::json!({
        "type": "text",
        "content": String::from_utf8_lossy(&content),
    }))
}

/// GET /file/status.
pub async fn file_status(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /mcp. TODO(integration): oc-mcp.
pub async fn mcp_status(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// POST /mcp.
pub async fn mcp_add(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// POST /mcp/:name/auth.
pub async fn mcp_auth_start(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// POST /mcp/:name/auth/callback.
pub async fn mcp_auth_callback(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// POST /mcp/:name/auth/authenticate.
pub async fn mcp_auth_authenticate(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// DELETE /mcp/:name/auth.
pub async fn mcp_auth_remove(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::json!({ "success": true }))
}

/// POST /mcp/:name/connect.
pub async fn mcp_connect(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /mcp/:name/disconnect.
pub async fn mcp_disconnect(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /sync/start.
pub async fn sync_start(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /sync/replay.
pub async fn sync_replay(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::json!({ "sessionID": "" }))
}

/// POST /sync/steal.
pub async fn sync_steal(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::json!({ "sessionID": "" }))
}

/// POST /sync/history.
pub async fn sync_history(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// POST /tui/*. These publish TUI control events; partial until oc-tui integration.
pub async fn tui_open_help(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_open_sessions(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_open_themes(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_open_models(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_submit_prompt(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_clear_prompt(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_append_prompt(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_execute_command(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_show_toast(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_publish(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_select_session(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_control_next(State(_state): State<crate::state::AppState>) -> HandlerResult {
    Ok((StatusCode::OK, axum::body::Body::empty()).into_response())
}

pub async fn tui_control_response(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// GET /experimental/capabilities.
pub async fn experimental_capabilities(
    State(_state): State<crate::state::AppState>,
) -> HandlerResult {
    json_value(serde_json::json!({ "backgroundSubagents": false }))
}

/// GET /experimental/console.
pub async fn experimental_console(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({ "consoleManagedProviders": [], "switchableOrgCount": 0 }))
}

/// GET /experimental/console/orgs.
pub async fn experimental_console_orgs(
    State(_state): State<crate::state::AppState>,
) -> HandlerResult {
    json_value(serde_json::json!({ "orgs": [] }))
}

/// POST /experimental/console/switch.
pub async fn experimental_console_switch(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// GET /experimental/tool.
pub async fn experimental_tool(
    State(_state): State<crate::state::AppState>,
    _query: Query<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /experimental/tool/ids.
pub async fn experimental_tool_ids(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /experimental/worktree.
pub async fn experimental_worktree_list(
    State(_state): State<crate::state::AppState>,
) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// POST /experimental/worktree.
pub async fn experimental_worktree_create(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Null)
}

/// DELETE /experimental/worktree.
pub async fn experimental_worktree_remove(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /experimental/worktree/reset.
pub async fn experimental_worktree_reset(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// GET /experimental/session.
pub async fn experimental_session_list(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let _include_archived = query.get("archived").map(|v| v == "true").unwrap_or(false);
    let stores = state.stores.read().await;
    let mut sessions = stores
        .sessions
        .values()
        .map(|r| {
            let mut info = v1_info(&r.info, &state.location.directory);
            let project = serde_json::json!({ "id": r.info.project_id, "worktree": r.info.location.directory });
            info.as_object_mut().unwrap().insert("project".into(), project);
            info
        })
        .collect::<Vec<_>>();
    drop(stores);
    sessions.sort_by(|a, b| {
        let at = a["time"]["updated"].as_i64().unwrap_or(0);
        let bt = b["time"]["updated"].as_i64().unwrap_or(0);
        bt.cmp(&at)
    });
    json_value(serde_json::Value::Array(sessions))
}

/// POST /experimental/session/:sessionID/background.
pub async fn experimental_session_background(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// GET /experimental/resource.
pub async fn experimental_resource(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// GET /experimental/workspace/adapter.
pub async fn workspace_adapters(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// GET /experimental/workspace.
pub async fn workspace_list(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// POST /experimental/workspace.
pub async fn workspace_create(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Null)
}

/// POST /experimental/workspace/sync-list.
pub async fn workspace_sync_list(State(_state): State<crate::state::AppState>) -> HandlerResult {
    no_content()
}

/// GET /experimental/workspace/status.
pub async fn workspace_status(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::Value::Array(Vec::new()))
}

/// DELETE /experimental/workspace/:id.
pub async fn workspace_remove(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Null)
}

/// POST /experimental/workspace/warp.
pub async fn workspace_warp(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    no_content()
}

/// POST /experimental/control-plane/move-session.
pub async fn control_plane_move_session(
    State(_state): State<crate::state::AppState>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    no_content()
}

/// PUT /auth/:providerID.
pub async fn control_auth_set(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// DELETE /auth/:providerID.
pub async fn control_auth_remove(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /log.
pub async fn control_log(
    State(_state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let level = body.get("level").and_then(|v| v.as_str()).unwrap_or("info");
    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    match level {
        "debug" => tracing::debug!("{message}"),
        "warn" => tracing::warn!("{message}"),
        "error" => tracing::error!("{message}"),
        _ => tracing::info!("{message}"),
    }
    json_value(serde_json::Value::Bool(true))
}

/// POST /experimental/project/:projectID/copy/generate-name.
pub async fn project_copy_generate_name(
    State(_state): State<crate::state::AppState>,
    _path: Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::json!({ "name": "copy" }))
}

/// GET /doc — OpenAPI document.
pub async fn openapi_doc(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(crate::openapi::document())
}

/// GET /openapi.json — OpenAPI document (alias).
pub async fn openapi_json(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(crate::openapi::document())
}
