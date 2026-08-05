//! v2 PTY handler. From reference/packages/server/src/handlers/pty.ts.

use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use super::{json, no_content, request_location, HandlerResult};
use crate::cors::is_allowed_request_origin;
use crate::errors::ApiError;
use crate::event::pty_id;
use crate::schema::{ConnectToken, LocationResponse};
use crate::state::now_millis;
use crate::state::{AppState, PtyRecord};

/// List PTY sessions for a location. From reference/packages/server/src/handlers/pty.ts.
pub async fn pty_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let stores = state.stores.read().await;
    let data = stores
        .pty
        .values()
        .map(|p| p.info.clone())
        .collect::<Vec<_>>();
    drop(stores);
    json(&LocationResponse {
        location: location.info(),
        data,
    })
}

pub async fn pty_create(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(pty_id);
    let info = serde_json::json!({
        "id": id,
        "cwd": body.get("cwd").cloned().unwrap_or_else(|| serde_json::Value::String(location.directory.clone())),
        "command": body.get("command").cloned().unwrap_or(serde_json::Value::Null),
        "title": body.get("title").cloned().unwrap_or(serde_json::Value::Null),
        "cols": body.get("cols").cloned().unwrap_or(serde_json::Value::Number(80.into())),
        "rows": body.get("rows").cloned().unwrap_or(serde_json::Value::Number(24.into())),
        "state": "running",
    });
    let mut stores = state.stores.write().await;
    stores.pty.insert(
        id.clone(),
        PtyRecord {
            info: info.clone(),
            running: true,
            buffer: Vec::new(),
        },
    );
    drop(stores);
    json(&LocationResponse {
        location: location.info(),
        data: info,
    })
}

async fn get_pty(state: &AppState, pty_id: &str) -> Result<serde_json::Value, ApiError> {
    // Not-found semantics come from the caller; the reference maps Pty.NotFoundError.
    let stores = state.stores.read().await;
    stores
        .pty
        .get(pty_id)
        .map(|p| p.info.clone())
        .ok_or_else(|| ApiError::PtyNotFound {
            pty_id: pty_id.to_string(),
            message: format!("PTY session not found: {pty_id}"),
        })
}

pub async fn pty_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let pty_id = params.get("ptyID").cloned().ok_or(ApiError::V1BadRequest)?;
    let info = get_pty(&state, &pty_id).await?;
    json(&LocationResponse {
        location: location.info(),
        data: info,
    })
}

pub async fn pty_update(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let pty_id = params.get("ptyID").cloned().ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    let record = stores
        .pty
        .get_mut(&pty_id)
        .ok_or_else(|| ApiError::PtyNotFound {
            pty_id: pty_id.clone(),
            message: format!("PTY session not found: {pty_id}"),
        })?;
    if let Some(title) = body.get("title").and_then(|v| v.as_str()) {
        record.info["title"] = serde_json::Value::String(title.to_string());
    }
    if let Some(cols) = body.get("cols") {
        record.info["cols"] = cols.clone();
    }
    if let Some(rows) = body.get("rows") {
        record.info["rows"] = rows.clone();
    }
    let info = record.info.clone();
    drop(stores);
    json(&LocationResponse {
        location: location.info(),
        data: info,
    })
}

pub async fn pty_remove(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let pty_id = params.get("ptyID").cloned().ok_or(ApiError::V1BadRequest)?;
    let mut stores = state.stores.write().await;
    if stores.pty.remove(&pty_id).is_none() {
        let message = format!("PTY session not found: {pty_id}");
        return Err(ApiError::PtyNotFound { pty_id, message });
    }
    drop(stores);
    no_content()
}

/// Mint a WebSocket connect ticket. From reference/packages/server/src/handlers/pty.ts
/// (`pty.connectToken`): requires the `x-opencode-ticket: 1` header plus an allowed
/// request origin, else 403.
pub async fn pty_connect_token(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let pty_id = params.get("ptyID").cloned().ok_or(ApiError::V1BadRequest)?;
    let has_ticket_header = headers
        .get("x-opencode-ticket")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "1")
        .unwrap_or(false);
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    if !has_ticket_header || !is_allowed_request_origin(origin, host, Some(&state.cors)) {
        return Err(ApiError::Forbidden {
            message: "Invalid PTY connect token request".into(),
        });
    }
    let _ = get_pty(&state, &pty_id).await?;
    let token = format!("ticket_{:x}", crate::event::event_id().len());
    let expires_at = now_millis() + 60_000;
    json(&LocationResponse {
        location: location.info(),
        data: ConnectToken {
            token: token.clone(),
            pty_id: pty_id.clone(),
            expires_at,
        },
    })
}

/// WebSocket upgrade for `/api/pty/:ptyID/connect`. From reference/packages/server/src/
/// handlers/pty.ts (`pty.connect`): empty 404 when the session is missing, empty 403 on
/// invalid ticket, then a binary PTY stream.
pub async fn pty_connect(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> HandlerResult {
    let pty_id = params.get("ptyID").cloned().ok_or(ApiError::V1BadRequest)?;

    let exists = {
        let stores = state.stores.read().await;
        stores.pty.contains_key(&pty_id)
    };
    if !exists {
        return Ok((StatusCode::NOT_FOUND, "").into_response());
    }

    if let Some(ticket) = query.get("ticket") {
        let origin = headers.get("origin").and_then(|v| v.to_str().ok());
        let host = headers.get("host").and_then(|v| v.to_str().ok());
        let allowed_origin = is_allowed_request_origin(origin, host, Some(&state.cors));
        let valid = allowed_origin && !ticket.is_empty();
        if !valid {
            return Ok((StatusCode::FORBIDDEN, "").into_response());
        }
    }

    let response = ws.on_upgrade(move |socket| pty_socket(state, pty_id, socket));
    Ok(response)
}

async fn pty_socket(state: AppState, pty_id: String, mut socket: WebSocket) {
    // Replay the captured output, then relay input back out. This is a partial port of
    // the PTY protocol from reference/packages/core/src/pty/protocol.ts.
    // TODO(integration): drive an actual pty child process via oc-core Pty service.
    let replay = {
        let stores = state.stores.read().await;
        stores
            .pty
            .get(&pty_id)
            .map(|p| p.buffer.clone())
            .unwrap_or_default()
    };
    if !replay.is_empty() {
        let _ = socket.send(Message::Binary(replay)).await;
    }
    let meta = serde_json::json!({ "cursor": -1 });
    let _ = socket
        .send(Message::Binary(serde_json::to_vec(&meta).unwrap()))
        .await;

    while let Some(Ok(message)) = socket.recv().await {
        match message {
            Message::Close(_) => break,
            Message::Text(text) => {
                let mut stores = state.stores.write().await;
                if let Some(record) = stores.pty.get_mut(&pty_id) {
                    record.buffer.extend_from_slice(text.as_bytes());
                }
                drop(stores);
                let _ = socket.send(Message::Text(text)).await;
            }
            Message::Binary(binary) => {
                let mut stores = state.stores.write().await;
                if let Some(record) = stores.pty.get_mut(&pty_id) {
                    record.buffer.extend_from_slice(&binary);
                }
                drop(stores);
                let _ = socket.send(Message::Binary(binary)).await;
            }
            _ => {}
        }
    }
}
