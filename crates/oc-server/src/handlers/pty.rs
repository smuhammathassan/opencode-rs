//! v2 PTY handler. From reference/packages/server/src/handlers/pty.ts.

use std::collections::HashMap;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;

use super::{json, no_content, request_location, HandlerResult};
use crate::cors::is_allowed_request_origin;
use crate::errors::ApiError;
use crate::event::pty_id;
use crate::schema::{ConnectToken, LocationResponse};
use crate::state::now_millis;
use crate::state::{AppState, PtyInput, PtyProcess, PtyRecord};

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
    if stores.pty.contains_key(&id) {
        return Err(ApiError::Conflict {
            message: format!("PTY session already exists: {id}"),
            resource: Some("pty".into()),
        });
    }
    stores.pty.insert(
        id.clone(),
        PtyRecord {
            info: info.clone(),
            running: true,
            buffer: Vec::new(),
            tickets: HashMap::new(),
        },
    );
    drop(stores);

    if let Err(error) = spawn_process(
        &state,
        &id,
        &location,
        body.get("command").and_then(|value| value.as_str()),
    )
    .await
    {
        state.stores.write().await.pty.remove(&id);
        return Err(error);
    }

    json(&LocationResponse {
        location: location.info(),
        data: info,
    })
}

/// Start the shell behind a PTY projection.
///
/// Unix uses a real master/slave pseudo-terminal so applications observe a
/// terminal (rather than three ordinary pipes), while other platforms retain
/// the portable piped fallback.
#[cfg(unix)]
async fn spawn_process(
    state: &AppState,
    pty_id: &str,
    location: &crate::location::Location,
    command: Option<&str>,
) -> Result<(), ApiError> {
    let (master, slave) = allocate_pty(80, 24)?;
    let master_input = duplicate_fd(&master)?;
    let master_output = duplicate_fd(&master)?;
    let slave_input = duplicate_fd(&slave)?;
    let slave_output = duplicate_fd(&slave)?;
    let slave_error = duplicate_fd(&slave)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut process = Command::new(&shell);
    if let Some(command) = command.filter(|command| !command.trim().is_empty()) {
        process.args(["-lc", command]);
    }
    process
        .current_dir(&location.directory)
        .envs(crate::pty_environment::pty_environment(location))
        .stdin(Stdio::from(std::fs::File::from(slave_input)))
        .stdout(Stdio::from(std::fs::File::from(slave_output)))
        .stderr(Stdio::from(std::fs::File::from(slave_error)));
    unsafe {
        process.as_std_mut().pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as libc::c_ulong, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = process.spawn().map_err(|error| ApiError::Unknown {
        message: format!("failed to start PTY shell: {error}"),
        reference: None,
    })?;
    drop(slave);

    let input = tokio::fs::File::from_std(std::fs::File::from(master_input));
    let output = tokio::fs::File::from_std(std::fs::File::from(master_output));
    let process = Arc::new(PtyProcess {
        child: Arc::new(Mutex::new(child)),
        stdin: Arc::new(Mutex::new(PtyInput::Native(input))),
        resize: Arc::new(master),
    });
    state
        .pty_processes
        .lock()
        .await
        .insert(pty_id.to_string(), process.clone());
    tokio::spawn(capture_output(state.clone(), pty_id.to_string(), output));

    let state_for_wait = state.clone();
    let pty_id_for_wait = pty_id.to_string();
    tokio::spawn(async move {
        let _ = process.child.lock().await.wait().await;
        {
            let mut processes = state_for_wait.pty_processes.lock().await;
            if processes
                .get(&pty_id_for_wait)
                .is_some_and(|current| Arc::ptr_eq(current, &process))
            {
                processes.remove(&pty_id_for_wait);
            }
        }
        let mut stores = state_for_wait.stores.write().await;
        if let Some(record) = stores.pty.get_mut(&pty_id_for_wait) {
            record.running = false;
            record.tickets.clear();
            record.info["state"] = serde_json::Value::String("exited".into());
        }
    });
    Ok(())
}

#[cfg(unix)]
fn allocate_pty(cols: u16, rows: u16) -> Result<(OwnedFd, OwnedFd), ApiError> {
    let mut master = -1;
    let mut slave = -1;
    let mut size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if result == -1 {
        return Err(ApiError::Unknown {
            message: format!(
                "failed to allocate PTY: {}",
                std::io::Error::last_os_error()
            ),
            reference: None,
        });
    }
    // `openpty` transfers ownership of both descriptors on success.
    Ok((unsafe { OwnedFd::from_raw_fd(master) }, unsafe {
        OwnedFd::from_raw_fd(slave)
    }))
}

#[cfg(unix)]
fn duplicate_fd(fd: &OwnedFd) -> Result<OwnedFd, ApiError> {
    let duplicate = unsafe { libc::dup(fd.as_raw_fd()) };
    if duplicate == -1 {
        return Err(ApiError::Unknown {
            message: format!(
                "failed to duplicate PTY descriptor: {}",
                std::io::Error::last_os_error()
            ),
            reference: None,
        });
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

/// Piped fallback used on platforms without a portable `openpty` API.
#[cfg(not(unix))]
async fn spawn_process(
    state: &AppState,
    pty_id: &str,
    location: &crate::location::Location,
    command: Option<&str>,
) -> Result<(), ApiError> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            "cmd.exe".to_string()
        } else {
            "/bin/sh".to_string()
        }
    });
    let mut process = Command::new(&shell);
    if let Some(command) = command.filter(|command| !command.trim().is_empty()) {
        if cfg!(windows) {
            process.args(["/C", command]);
        } else {
            process.args(["-lc", command]);
        }
    }
    process
        .current_dir(&location.directory)
        .envs(crate::pty_environment::pty_environment(location))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = process.spawn().map_err(|error| ApiError::Unknown {
        message: format!("failed to start PTY shell: {error}"),
        reference: None,
    })?;
    let stdin = child.stdin.take().ok_or_else(|| ApiError::Unknown {
        message: "PTY shell did not expose stdin".into(),
        reference: None,
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let process = Arc::new(PtyProcess {
        child: Arc::new(Mutex::new(child)),
        stdin: Arc::new(Mutex::new(PtyInput::Pipe(stdin))),
    });
    state
        .pty_processes
        .lock()
        .await
        .insert(pty_id.to_string(), process.clone());

    if let Some(stdout) = stdout {
        tokio::spawn(capture_output(state.clone(), pty_id.to_string(), stdout));
    }
    if let Some(stderr) = stderr {
        tokio::spawn(capture_output(state.clone(), pty_id.to_string(), stderr));
    }

    let state_for_wait = state.clone();
    let pty_id_for_wait = pty_id.to_string();
    tokio::spawn(async move {
        let _ = process.child.lock().await.wait().await;
        {
            let mut processes = state_for_wait.pty_processes.lock().await;
            if processes
                .get(&pty_id_for_wait)
                .is_some_and(|current| Arc::ptr_eq(current, &process))
            {
                processes.remove(&pty_id_for_wait);
            }
        }
        let mut stores = state_for_wait.stores.write().await;
        if let Some(record) = stores.pty.get_mut(&pty_id_for_wait) {
            record.running = false;
            record.tickets.clear();
            record.info["state"] = serde_json::Value::String("exited".into());
        }
    });
    Ok(())
}

async fn capture_output<R>(state: AppState, pty_id: String, mut reader: R)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut chunk = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(read) => read,
            Err(error) => {
                tracing::debug!(%pty_id, ?error, "PTY output stream closed with an error");
                break;
            }
        };
        if read == 0 {
            break;
        }
        if let Some(record) = state.stores.write().await.pty.get_mut(&pty_id) {
            record.buffer.extend_from_slice(&chunk[..read]);
        } else {
            break;
        }
    }
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
    #[cfg(unix)]
    if let (Some(cols), Some(rows)) = (
        info.get("cols").and_then(|value| value.as_u64()),
        info.get("rows").and_then(|value| value.as_u64()),
    ) {
        if let Some(process) = state.pty_processes.lock().await.get(&pty_id).cloned() {
            resize_pty(&process.resize, cols, rows)?;
        }
    }
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
    if let Some(process) = state.pty_processes.lock().await.remove(&pty_id) {
        let _ = process.child.lock().await.kill().await;
    }
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
    let token = format!("ticket_{}", crate::event::event_id());
    let expires_at = now_millis() + 60_000;
    {
        let mut stores = state.stores.write().await;
        if let Some(record) = stores.pty.get_mut(&pty_id) {
            record.tickets.insert(token.clone(), expires_at);
        }
    }
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
        let valid = if allowed_origin && !ticket.is_empty() {
            let now = now_millis();
            let mut stores = state.stores.write().await;
            stores
                .pty
                .get_mut(&pty_id)
                .and_then(|record| record.tickets.remove(ticket))
                .is_some_and(|expires_at| expires_at > now)
        } else {
            false
        };
        if !valid {
            return Ok((StatusCode::FORBIDDEN, "").into_response());
        }
    }

    let response = ws.on_upgrade(move |socket| pty_socket(state, pty_id, socket));
    Ok(response)
}

async fn pty_socket(state: AppState, pty_id: String, mut socket: WebSocket) {
    // Replay captured output, then stream newly captured process output while
    // forwarding client input to the shell. This is a partial port of the PTY
    // protocol from reference/packages/core/src/pty/protocol.ts.
    let replay = {
        let stores = state.stores.read().await;
        stores
            .pty
            .get(&pty_id)
            .map(|p| p.buffer.clone())
            .unwrap_or_default()
    };
    let mut cursor = replay.len();
    if !replay.is_empty() {
        let _ = socket.send(Message::Binary(replay.into())).await;
    }
    let meta = serde_json::json!({ "cursor": -1 });
    let _ = socket
        .send(Message::Binary(serde_json::to_vec(&meta).unwrap().into()))
        .await;

    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(40));
    loop {
        tokio::select! {
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break };
                match message {
                    Message::Close(_) => break,
                    Message::Text(text) => {
                        if !write_input(&state, &pty_id, text.as_bytes()).await {
                            break;
                        }
                    }
                    Message::Binary(binary) => {
                        if !write_input(&state, &pty_id, &binary).await {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            _ = ticker.tick() => {
                let chunk = {
                    let stores = state.stores.read().await;
                    stores.pty.get(&pty_id).and_then(|record| {
                        (cursor < record.buffer.len()).then(|| record.buffer[cursor..].to_vec())
                    })
                };
                if let Some(chunk) = chunk {
                    cursor += chunk.len();
                    if socket.send(Message::Binary(chunk.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

async fn write_input(state: &AppState, pty_id: &str, input: &[u8]) -> bool {
    let process = state.pty_processes.lock().await.get(pty_id).cloned();
    let Some(process) = process else { return false };
    let result = match &mut *process.stdin.lock().await {
        #[cfg(unix)]
        PtyInput::Native(stdin) => stdin.write_all(input).await.is_ok(),
        #[cfg(not(unix))]
        PtyInput::Pipe(stdin) => stdin.write_all(input).await.is_ok(),
    };
    result
}

#[cfg(unix)]
fn resize_pty(fd: &OwnedFd, cols: u64, rows: u64) -> Result<(), ApiError> {
    let size = libc::winsize {
        ws_row: rows.clamp(1, u16::MAX as u64) as u16,
        ws_col: cols.clamp(1, u16::MAX as u64) as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCSWINSZ as libc::c_ulong, &size) };
    if result == -1 {
        return Err(ApiError::Unknown {
            message: format!("failed to resize PTY: {}", std::io::Error::last_os_error()),
            reference: None,
        });
    }
    Ok(())
}
