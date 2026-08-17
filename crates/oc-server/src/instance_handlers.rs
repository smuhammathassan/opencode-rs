//! v1 instance handlers.
//!
//! Port of the legacy instance surface from
//! reference/packages/opencode/src/server/routes/instance/httpapi/handlers/*. Many
//! routes depend on oc-core services that are not integrated yet and return stable
//! empty/default shapes. TODO(integration): wire each group to its oc-* service.

use std::collections::HashMap;
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use serde_json::{json, Value};

use crate::errors::ApiError;
use crate::handlers::{json_value, no_content, HandlerResult};
use crate::schema::{LocationRef, ModelRef, SessionInfo};
use crate::state::{now_millis, AppState};

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

/// GET /config. From reference/packages/opencode/src/server/routes/instance/httpapi/
/// handlers/config.ts (`config.get`). The production listener seeds this
/// projection from `oc-config`; tests/embedders may still provide their own.
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
    let updated = body.0;
    let path = project_config_path(&state.location.directory);
    write_project_config(&path, &updated).map_err(|error| ApiError::Unknown {
        message: format!("failed to persist project config: {error}"),
        reference: None,
    })?;
    let mut stores = state.stores.write().await;
    stores.config = updated;
    let config = stores.config.clone();
    drop(stores);
    state.emit_event(crate::event::Event {
        id: crate::event::event_id(),
        metadata: None,
        r#type: "config.updated".into(),
        durable: None,
        location: Some(state.location.reference()),
        data: config.clone(),
    });
    json_value(config)
}

/// GET /config/providers. From reference/.../handlers/config.ts (`config.providers`).
pub async fn config_providers(State(state): State<crate::state::AppState>) -> HandlerResult {
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    let providers = crate::handlers::provider::provider_values_from_state_config(&state, &config);
    json_value(serde_json::json!({ "providers": providers, "default": {} }))
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
    let updated = body.0;
    write_global_config(&updated).map_err(|error| ApiError::Unknown {
        message: format!("failed to persist global config: {error}"),
        reference: None,
    })?;
    let mut stores = state.stores.write().await;
    stores.config = updated;
    let config = stores.config.clone();
    drop(stores);
    state.emit_event(crate::event::Event {
        id: crate::event::event_id(),
        metadata: None,
        r#type: "config.updated".into(),
        durable: None,
        location: None,
        data: config.clone(),
    });
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

/// GET /vcs. From the reference instance VCS handler.
pub async fn vcs_get(State(state): State<crate::state::AppState>) -> HandlerResult {
    let inside = git_output(
        &state.location.directory,
        &["rev-parse", "--is-inside-work-tree"],
    )
    .await
    .map(|output| output.trim() == "true")
    .unwrap_or(false);
    json_value(serde_json::json!({
        "command": "git",
        "state": { "mode": if inside { "git" } else { "no-git" } },
    }))
}

/// GET /vcs/status.
pub async fn vcs_status(State(state): State<crate::state::AppState>) -> HandlerResult {
    let output = git_output(
        &state.location.directory,
        &["status", "--short", "--untracked-files=all"],
    )
    .await
    .unwrap_or_default();
    let rows = output
        .lines()
        .filter_map(|line| {
            let status = line.get(..2)?.trim().to_string();
            let path = line.get(3..)?.trim().to_string();
            if path.is_empty() {
                return None;
            }
            Some(serde_json::json!({ "path": path, "status": status }))
        })
        .collect::<Vec<_>>();
    json_value(serde_json::Value::Array(rows))
}

/// GET /vcs/diff.
pub async fn vcs_diff(
    State(state): State<crate::state::AppState>,
    query: Query<HashMap<String, String>>,
) -> HandlerResult {
    let args = ["diff", "--no-ext-diff", "--no-color"];
    if let Some(path) = query.get("path") {
        let safe = safe_workspace_path(&state.location.directory, path).ok_or_else(|| {
            ApiError::ApiNotFound {
                message: "Not found".into(),
            }
        })?;
        let relative = safe
            .strip_prefix(&state.location.directory)
            .unwrap_or(&safe)
            .to_string_lossy()
            .to_string();
        // Keep the path in a separate owned buffer so it remains valid while
        // the async command is assembled.
        let output = git_diff_for_path(&state.location.directory, &relative).await;
        return json_value(parse_git_diff(&output));
    }
    let output = git_output(&state.location.directory, &args)
        .await
        .unwrap_or_default();
    json_value(parse_git_diff(&output))
}

/// GET /vcs/diff/raw.
pub async fn vcs_diff_raw(
    State(state): State<crate::state::AppState>,
    query: Query<HashMap<String, String>>,
) -> HandlerResult {
    let output = if let Some(path) = query.get("path") {
        let safe = safe_workspace_path(&state.location.directory, path).ok_or_else(|| {
            ApiError::ApiNotFound {
                message: "Not found".into(),
            }
        })?;
        let relative = safe
            .strip_prefix(&state.location.directory)
            .unwrap_or(&safe)
            .to_string_lossy()
            .to_string();
        git_diff_for_path(&state.location.directory, &relative).await
    } else {
        git_output(
            &state.location.directory,
            &["diff", "--no-ext-diff", "--no-color"],
        )
        .await
        .unwrap_or_default()
    };
    Ok(([("content-type", "text/x-diff; charset=utf-8")], output).into_response())
}

/// POST /vcs/apply. Applies a unified diff through `git apply` after checking
/// that every referenced path stays inside the active worktree.
pub async fn vcs_apply(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let patch = body
        .get("patch")
        .or_else(|| body.get("diff"))
        .and_then(|value| value.as_str())
        .ok_or(ApiError::V1BadRequest)?;
    validate_patch_paths(patch).ok_or_else(|| ApiError::ApiNotFound {
        message: "Patch contains a path outside the worktree".into(),
    })?;
    let mut command = tokio::process::Command::new("git");
    command
        .args(["apply", "--whitespace=nowarn"])
        .current_dir(&state.location.directory)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(|error| ApiError::Unknown {
        message: format!("failed to start git apply: {error}"),
        reference: None,
    })?;
    if let Some(stdin) = child.stdin.as_mut() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(patch.as_bytes())
            .await
            .map_err(|error| ApiError::Unknown {
                message: format!("failed to send patch to git: {error}"),
                reference: None,
            })?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("git apply failed: {error}"),
            reference: None,
        })?;
    if !output.status.success() {
        return Err(ApiError::Unknown {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            reference: None,
        });
    }
    json_value(serde_json::json!({ "success": true }))
}

fn validate_patch_paths(patch: &str) -> Option<()> {
    for line in patch.lines() {
        let Some(raw) = line
            .strip_prefix("--- ")
            .or_else(|| line.strip_prefix("+++ "))
        else {
            continue;
        };
        let path = raw
            .split_once('\t')
            .map(|(path, _)| path)
            .unwrap_or(raw)
            .strip_prefix("a/")
            .or_else(|| raw.strip_prefix("b/"))
            .unwrap_or(raw);
        if path == "/dev/null" {
            continue;
        }
        let candidate = FsPath::new(path);
        if candidate.is_absolute()
            || candidate.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return None;
        }
    }
    Some(())
}

async fn git_output(directory: &str, args: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn git_diff_for_path(directory: &str, path: &str) -> String {
    git_output(
        directory,
        &["diff", "--no-ext-diff", "--no-color", "--", path],
    )
    .await
    .unwrap_or_default()
}

fn parse_git_diff(diff: &str) -> serde_json::Value {
    let mut rows = Vec::new();
    let mut current_file: Option<String> = None;
    let mut current_patch = String::new();
    let mut additions = 0u64;
    let mut deletions = 0u64;

    let flush = |rows: &mut Vec<serde_json::Value>,
                 file: &mut Option<String>,
                 patch: &mut String,
                 additions: &mut u64,
                 deletions: &mut u64| {
        if let Some(file) = file.take() {
            rows.push(serde_json::json!({
                "file": file,
                "status": "modified",
                "additions": *additions,
                "deletions": *deletions,
                "patch": patch,
            }));
        }
        patch.clear();
        *additions = 0;
        *deletions = 0;
    };

    for line in diff.lines() {
        if let Some(file) = line.strip_prefix("+++ b/") {
            flush(
                &mut rows,
                &mut current_file,
                &mut current_patch,
                &mut additions,
                &mut deletions,
            );
            current_file = Some(file.to_string());
        }
        if current_file.is_some() {
            current_patch.push_str(line);
            current_patch.push('\n');
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
    }
    flush(
        &mut rows,
        &mut current_file,
        &mut current_patch,
        &mut additions,
        &mut deletions,
    );
    serde_json::Value::Array(rows)
}

/// GET /command. From reference/.../groups/instance.ts (`command.list`).
pub async fn command_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let mut registry = oc_command::command::Registry::new(&state.location.directory);
    if let Ok(entries) =
        oc_command::command::load_from_dir(std::path::Path::new(&state.location.directory))
    {
        registry.add_config_entries(entries);
    }
    add_mcp_prompt_commands(&state, &mut registry).await;
    let data = registry
        .list()
        .filter_map(|command| serde_json::to_value(command).ok())
        .collect::<Vec<_>>();
    json_value(serde_json::Value::Array(data))
}

/// GET /agent. From reference/.../groups/instance.ts (`app.agents`).
pub async fn agent_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    let mut agents = vec![serde_json::json!({
        "name": "build",
        "description": "General-purpose coding agent",
        "mode": "primary",
        "native": true,
        "hidden": false,
        "permission": [],
        "options": {}
    })];
    if let Some(configured) = config.get("agent").and_then(serde_json::Value::as_object) {
        for (name, value) in configured {
            if name == "build" {
                continue;
            }
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() {
                object.insert("name".into(), serde_json::Value::String(name.clone()));
                object
                    .entry("mode")
                    .or_insert_with(|| serde_json::Value::String("primary".into()));
                object
                    .entry("native")
                    .or_insert(serde_json::Value::Bool(false));
                object
                    .entry("hidden")
                    .or_insert(serde_json::Value::Bool(false));
                object
                    .entry("options")
                    .or_insert_with(|| serde_json::json!({}));
            }
            agents.push(value);
        }
    }
    json_value(serde_json::Value::Array(agents))
}

/// GET /skill. From reference/.../groups/instance.ts (`app.skills`).
pub async fn skill_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"));
    let settings = oc_command::skill::Settings {
        home,
        directory: std::path::PathBuf::from(&state.location.directory),
        worktree: std::path::PathBuf::from(&state.location.directory),
        disable_external_skills: false,
        disable_claude_code_skills: false,
        paths: Vec::new(),
        pulled_dirs: Vec::new(),
        config_dirs: None,
    };
    let data = oc_command::skill::SkillService::load_with_environment(&settings)
        .map(|service| {
            service
                .all()
                .into_iter()
                .filter_map(|skill| serde_json::to_value(skill).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut data = data;
    data.extend(crate::plugin_registry::plugin_skill_values(&state));
    json_value(serde_json::Value::Array(data))
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
        "revert": info.revert.clone(),
    })
}

fn set_title_via_session_service(info: &SessionInfo, title: &str) -> String {
    let mut session = oc_session::v1::SessionInfo::default();
    session.title = info.title.clone();
    oc_session::service::SessionMutationService
        .set_title(&session, title)
        .title
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
    let limit = query.get("limit").and_then(|v| v.parse::<usize>().ok());
    limit_messages(&mut messages, limit);
    drop(stores);
    json_value(serde_json::Value::Array(messages))
}

/// Apply the reference message endpoint's limit to the newest messages while
/// preserving chronological order. TUI replay then can safely request a
/// bounded page and retain the latest context.
fn limit_messages(messages: &mut Vec<Value>, limit: Option<usize>) {
    if let Some(limit) = limit {
        let keep_from = messages.len().saturating_sub(limit);
        messages.drain(..keep_from);
    }
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
    state.persist_session(&info);
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
        record.info.title = set_title_via_session_service(&record.info, title);
    }
    record.info.time.updated = now_millis();
    let persisted_info = record.info.clone();
    let info = v1_info(&record.info, &state.location.directory);
    drop(stores);
    state.persist_session(&persisted_info);
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
    state.delete_session(&session_id);
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
    state.cancel_session_run(&session_id).await;
    state.emit_event(crate::event::Event {
        id: crate::event::event_id(),
        metadata: None,
        r#type: "session.status".into(),
        durable: None,
        location: None,
        data: serde_json::json!({
            "sessionID": session_id,
            "status": { "type": "idle" },
        }),
    });
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
    let text = prompt_text(&body);
    let delivery = body
        .get("delivery")
        .and_then(|value| value.as_str())
        .filter(|delivery| matches!(*delivery, "steer" | "queue"))
        .unwrap_or("steer")
        .to_string();
    let model = body
        .get("model")
        .filter(|value| !value.is_null())
        .map(model_from_value);
    let mut message = serde_json::json!({
        "id": message_id,
        "time": { "created": created },
        "type": "user",
        "text": text,
        "files": [],
        "agents": [],
    });
    if let Some(files) = body.get("files").and_then(|value| value.as_array()) {
        message["files"] = serde_json::Value::Array(files.clone());
    }
    if let Some(agents) = body.get("agents").and_then(|value| value.as_array()) {
        message["agents"] = serde_json::Value::Array(agents.clone());
    }
    if let Some(metadata) = body.get("metadata") {
        message["metadata"] = metadata.clone();
    }
    if let Some(parts) = body.get("parts").and_then(|value| value.as_array()) {
        message["content"] = serde_json::Value::Array(parts.clone());
    }
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    record.messages.push(message.clone());
    if let Some(agent) = body.get("agent").and_then(|value| value.as_str()) {
        record.info.agent = Some(agent.to_string());
    }
    if let Some(model) = model {
        record.info.model = Some(model);
    }
    record.info.time.updated = created;
    record.active = true;
    let admitted_seq = record.messages.len() as u64;
    let persisted_info = record.info.clone();
    drop(stores);
    state.persist_session(&persisted_info);
    state.persist_message(&session_id, &message);
    state
        .enqueue_session_input(
            &session_id,
            message_id.clone(),
            json!({ "text": text }),
            admitted_seq,
            delivery.clone(),
        )
        .await;
    let prompt_event = json!({
        "timestamp": created,
        "sessionID": session_id,
        "messageID": message_id,
        "prompt": { "text": text },
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

    crate::runner::schedule_session_run(state.clone(), session_id.clone());

    json_value(serde_json::json!({
        "info": {
            "id": message_id,
            "role": "user",
            "time": { "created": created },
        },
        "parts": [message],
    }))
}

fn model_from_value(value: &serde_json::Value) -> crate::schema::ModelRef {
    if let Some(model) = value.as_str() {
        let (provider_id, id) = model
            .split_once('/')
            .map(|(provider, model_id)| (provider, model_id))
            .unwrap_or(("", model));
        return crate::schema::ModelRef {
            id: id.to_string(),
            provider_id: provider_id.to_string(),
            variant: None,
        };
    }
    crate::schema::ModelRef {
        id: value
            .get("modelID")
            .or_else(|| value.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        provider_id: value
            .get("providerID")
            .or_else(|| value.get("providerId"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        variant: value
            .get("variant")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

/// Accept both the v1 `parts` request used by the TUI/CLI and the older
/// shorthand `{prompt: "..."}` shape used by integrations.
fn prompt_text(body: &serde_json::Value) -> String {
    if let Some(parts) = body.get("parts").and_then(|v| v.as_array()) {
        let text = parts
            .iter()
            .filter_map(|part| {
                if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                    part.get("text").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return text;
        }
    }
    body.get("prompt")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("prompt")
                .and_then(|v| v.get("text"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| body.get("text").and_then(|v| v.as_str()))
        .unwrap_or_default()
        .to_string()
}

/// Resolve a configured provider route for the production session-runner
/// adapter. Keeping this in one place means provider auth/base-URL behavior
/// cannot drift between the compatibility surfaces and the live runner.
#[allow(dead_code)]
pub(crate) fn configured_model(provider: &str, model_id: &str) -> Result<oc_llm::Model, String> {
    configured_model_with_auth(provider, model_id, None)
}

pub(crate) fn configured_model_with_auth(
    provider: &str,
    model_id: &str,
    auth: Option<oc_llm::route::auth::Auth>,
) -> Result<oc_llm::Model, String> {
    match provider {
        "openai" => {
            let config = oc_llm::providers::openai::Config {
                base_url: std::env::var("OPENCODE_OPENAI_BASE_URL")
                    .ok()
                    .or_else(|| std::env::var("OPENAI_BASE_URL").ok()),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::openai::configure(config).model(model_id))
        }
        "anthropic" => {
            let config = oc_llm::providers::anthropic::Config {
                base_url: std::env::var("OPENCODE_ANTHROPIC_BASE_URL")
                    .ok()
                    .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok()),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::anthropic::configure(config).model(model_id))
        }
        "google" => {
            let config = oc_llm::providers::google::Config {
                base_url: std::env::var("OPENCODE_GOOGLE_BASE_URL").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::google::configure(config).model(model_id))
        }
        "openrouter" => {
            let config = oc_llm::providers::openrouter::Config {
                base_url: std::env::var("OPENCODE_OPENROUTER_BASE_URL").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::openrouter::configure(config).model(model_id))
        }
        "xai" => {
            let config = oc_llm::providers::xai::Config {
                base_url: std::env::var("OPENCODE_XAI_BASE_URL").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::xai::configure(config).model(model_id))
        }
        "azure" => {
            let config = oc_llm::providers::azure::Config {
                url: oc_llm::providers::azure::AzureUrl {
                    resource_name: std::env::var("AZURE_RESOURCE_NAME").ok(),
                    base_url: std::env::var("OPENCODE_AZURE_BASE_URL")
                        .ok()
                        .or_else(|| std::env::var("AZURE_OPENAI_ENDPOINT").ok()),
                },
                api_version: std::env::var("AZURE_API_VERSION").ok(),
                api_key: std::env::var("AZURE_OPENAI_API_KEY").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::azure::configure(config).model(model_id))
        }
        "cloudflare-ai-gateway" => {
            let config = oc_llm::providers::cloudflare::AIGatewayOptions {
                url: oc_llm::providers::cloudflare::GatewayUrl {
                    account_id: std::env::var("CLOUDFLARE_ACCOUNT_ID").ok(),
                    base_url: std::env::var("OPENCODE_CLOUDFLARE_AI_GATEWAY_BASE_URL").ok(),
                    gateway_id: std::env::var("CLOUDFLARE_AI_GATEWAY_ID").ok(),
                },
                api_key: std::env::var("CLOUDFLARE_API_KEY").ok(),
                gateway_api_key: std::env::var("CF_AIG_TOKEN").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::cloudflare::configure_ai_gateway(config).model(model_id))
        }
        "cloudflare-workers-ai" => {
            let config = oc_llm::providers::cloudflare::WorkersAIOptions {
                account_id: std::env::var("CLOUDFLARE_ACCOUNT_ID").ok(),
                base_url: std::env::var("OPENCODE_CLOUDFLARE_WORKERS_AI_BASE_URL").ok(),
                api_key: std::env::var("CLOUDFLARE_API_KEY").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::cloudflare::configure_workers_ai(config).model(model_id))
        }
        "github-copilot" => {
            let config = oc_llm::providers::github_copilot::Config {
                base_url: std::env::var("OPENCODE_GITHUB_COPILOT_BASE_URL")
                    .ok()
                    .or_else(|| std::env::var("GITHUB_COPILOT_BASE_URL").ok())
                    .unwrap_or_else(|| "https://api.githubcopilot.com".to_string()),
                api_key: std::env::var("GITHUB_COPILOT_TOKEN").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::github_copilot::configure(config).model(model_id))
        }
        "amazon-bedrock" => {
            let credentials = oc_llm::providers::amazon_bedrock::BedrockCredentials {
                region: std::env::var("AWS_REGION")
                    .ok()
                    .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok()),
                access_key_id: std::env::var("AWS_ACCESS_KEY_ID").ok(),
                secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
                session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
            };
            let credentials = (!credentials.access_key_id.is_none()
                || !credentials.secret_access_key.is_none()
                || !credentials.region.is_none()
                || !credentials.session_token.is_none())
            .then_some(credentials);
            let config = oc_llm::providers::amazon_bedrock::Config {
                api_key: std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok(),
                region: std::env::var("AWS_REGION")
                    .ok()
                    .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok()),
                base_url: std::env::var("OPENCODE_BEDROCK_BASE_URL").ok(),
                credentials,
                ..Default::default()
            };
            Ok(oc_llm::providers::amazon_bedrock::configure(config).model(model_id))
        }
        "openai-compatible" => {
            let base_url = std::env::var("OPENCODE_OPENAI_BASE_URL")
                .ok()
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
                .ok_or_else(|| {
                    "provider `openai-compatible` requires OPENCODE_OPENAI_BASE_URL".to_string()
                })?;
            let config = oc_llm::providers::openai_compatible::GenericModelOptions {
                base_url,
                api_key: std::env::var("OPENAI_API_KEY").ok(),
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::openai_compatible::configure(config).model(model_id))
        }
        "opencode" | "opencode-go" => {
            // OpenCode Zen / OpenCode Go are OpenAI-compatible endpoints from
            // the models.dev catalog (`api` field). The reference reaches them
            // through its built-in `opencode` default plugin; the native runner
            // mirrors the same wire shape so catalog models are executable.
            let base_url = std::env::var("OPENCODE_OPENCODE_BASE_URL")
                .ok()
                .unwrap_or_else(|| {
                    if provider == "opencode-go" {
                        "https://opencode.ai/zen/go/v1".to_string()
                    } else {
                        "https://opencode.ai/zen/v1".to_string()
                    }
                });
            let api_key = std::env::var("OPENCODE_API_KEY")
                .ok()
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    // The reference `opencode` default plugin sends `apiKey:
                    // "public"` when no credential is configured so free-tier
                    // catalog models remain reachable; paid models are disabled
                    // at catalog load time in the reference.
                    if provider == "opencode" {
                        Some("public".to_string())
                    } else {
                        None
                    }
                });
            let config = oc_llm::providers::openai_compatible::GenericModelOptions {
                base_url,
                api_key,
                auth,
                ..Default::default()
            };
            Ok(oc_llm::providers::openai_compatible::configure(config).model(model_id))
        }
        other => {
            return Err(format!(
                "provider `{other}` is not wired for the local text runner"
            ))
        }
    }
}

/// Resolve a model using a custom provider declared in `opencode.json` before
/// falling back to the built-in provider facades above. Custom providers use
/// the OpenAI-compatible wire shape when their config supplies a base URL.
#[allow(dead_code)]
pub(crate) fn configured_model_for_config(
    config: &serde_json::Value,
    provider: &str,
    model_id: &str,
) -> Result<oc_llm::Model, String> {
    configured_model_for_config_with_auth(config, provider, model_id, None)
}

pub(crate) fn configured_model_for_config_with_auth(
    config: &serde_json::Value,
    provider: &str,
    model_id: &str,
    auth: Option<oc_llm::route::auth::Auth>,
) -> Result<oc_llm::Model, String> {
    configured_model_for_config_with_auth_and_base(config, provider, model_id, auth, None)
}

/// Resolve a model with an optional transport base URL supplied by a native
/// auth integration. OpenAI Codex OAuth uses the ChatGPT backend endpoint,
/// while ordinary API-key OpenAI traffic keeps the standard API endpoint.
pub(crate) fn configured_model_for_config_with_auth_and_base(
    config: &serde_json::Value,
    provider: &str,
    model_id: &str,
    auth: Option<oc_llm::route::auth::Auth>,
    native_base_url: Option<&str>,
) -> Result<oc_llm::Model, String> {
    let Some(provider_config) = config
        .get("provider")
        .and_then(serde_json::Value::as_object)
        .and_then(|providers| providers.get(provider))
    else {
        if let Some(base_url) = native_base_url {
            match provider {
                "openai" => {
                    let config = oc_llm::providers::openai::Config {
                        base_url: Some(base_url.to_string()),
                        auth,
                        ..Default::default()
                    };
                    return Ok(oc_llm::providers::openai::configure(config).model(model_id));
                }
                "github-copilot" => {
                    let config = oc_llm::providers::github_copilot::Config {
                        base_url: base_url.to_string(),
                        auth,
                        ..Default::default()
                    };
                    return Ok(oc_llm::providers::github_copilot::configure(config).model(model_id));
                }
                _ => {}
            }
        }
        return configured_model_with_auth(provider, model_id, auth);
    };
    let options = provider_config
        .get("options")
        .and_then(serde_json::Value::as_object);
    let base_url = native_base_url.or_else(|| {
        options
            .and_then(|options| options.get("baseURL").or_else(|| options.get("baseUrl")))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                provider_config
                    .get("api")
                    .and_then(serde_json::Value::as_str)
                    .filter(|api| api.starts_with("http://") || api.starts_with("https://"))
            })
    });
    let Some(base_url) = base_url else {
        return configured_model_with_auth(provider, model_id, auth);
    };
    let api_key = options
        .and_then(|options| options.get("apiKey"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            provider_config
                .get("env")
                .and_then(serde_json::Value::as_array)
                .and_then(|names| {
                    names
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
                })
        });
    let headers = options
        .and_then(|options| options.get("headers"))
        .and_then(serde_json::Value::as_object)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        });
    if provider == "github-copilot" {
        let config = oc_llm::providers::github_copilot::Config {
            base_url: base_url.to_string(),
            api_key,
            auth,
            headers,
            ..Default::default()
        };
        return Ok(oc_llm::providers::github_copilot::configure(config).model(model_id));
    }
    let model = oc_llm::providers::openai_compatible::configure(
        oc_llm::providers::openai_compatible::GenericModelOptions {
            provider: Some(provider.to_string()),
            base_url: base_url.to_string(),
            api_key,
            auth,
            headers,
            ..Default::default()
        },
    )
    .model(model_id);
    Ok(model)
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
    let command_name = body
        .get("command")
        .and_then(|value| value.as_str())
        .map(|name| name.trim_start_matches('/'))
        .filter(|name| !name.is_empty())
        .ok_or(ApiError::V1BadRequest)?;
    let arguments = body
        .get("arguments")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    let registry = command_registry(&state).await?;
    let mcp_prompt = registry.get_mcp_prompt(command_name).cloned();
    let command = registry
        .get(command_name)
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("Command not found: {command_name}"),
        })?
        .clone();
    let rendered = if let Some(prompt) = mcp_prompt {
        let template = resolve_mcp_prompt(&state, &prompt).await?;
        oc_command::command::render(&template, arguments)
    } else {
        command.render(arguments)
    };
    let expanded = oc_command::command::expand_shell(&rendered, &|shell_command| {
        run_command_shell(&state.location.directory, shell_command)
    })
    .unwrap_or_default()
    .trim()
    .to_string();

    let mut prompt = body.0;
    prompt["parts"] = serde_json::Value::Array(command_prompt_parts(&prompt, expanded));
    // Command metadata supplies defaults, while explicit request fields win.
    if prompt.get("agent").is_none() {
        if let Some(agent) = command.agent.clone() {
            prompt["agent"] = serde_json::Value::String(agent);
        }
    }
    if prompt.get("model").is_none() {
        if let Some(model) = command.model.clone() {
            prompt["model"] = serde_json::Value::String(model);
        }
    }

    session_prompt(State(state), Path(params), axum::extract::Json(prompt)).await
}

/// Discover prompts from the live MCP clients and expose them using the
/// reference's sanitized `client:prompt` command keys.
pub(crate) async fn add_mcp_prompt_commands(
    state: &AppState,
    registry: &mut oc_command::command::Registry,
) {
    let config = state.stores.read().await.config.clone();
    let clients = state.mcp_clients.lock().await.clone();
    let mut prompts = Vec::new();

    for (client_name, client) in clients {
        let timeout = mcp_request_timeout(&config, &client_name);
        let Ok(listed) = oc_mcp::catalog::prompts(client, Some(timeout)).await else {
            continue;
        };
        for prompt in listed {
            let command_name = format!(
                "{}:{}",
                oc_mcp::catalog::sanitize(&client_name),
                oc_mcp::catalog::sanitize(&prompt.name)
            );
            let arguments = prompt
                .arguments
                .unwrap_or_default()
                .into_iter()
                .map(|argument| argument.name)
                .collect();
            prompts.push(oc_command::command::McpPrompt {
                command_name,
                client: client_name.clone(),
                name: prompt.name,
                description: prompt.description,
                arguments,
            });
        }
    }
    registry.add_mcp_prompts(prompts);
}

/// Resolve an MCP prompt template using `$1`, `$2`, ... placeholders, then
/// let the regular command renderer substitute the user's actual arguments.
async fn resolve_mcp_prompt(
    state: &AppState,
    prompt: &oc_command::command::McpPrompt,
) -> Result<String, ApiError> {
    let client = state
        .mcp_clients
        .lock()
        .await
        .get(&prompt.client)
        .cloned()
        .ok_or_else(|| ApiError::Unknown {
            message: format!("MCP client {} is not connected", prompt.client),
            reference: None,
        })?;
    let config = state.stores.read().await.config.clone();
    let timeout = mcp_request_timeout(&config, &prompt.client);
    let result = client
        .get_prompt(&prompt.name, Some(prompt.request_arguments()), timeout)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("MCP prompts/get failed for {}: {error}", prompt.name),
            reference: None,
        })?;
    Ok(result
        .messages
        .iter()
        .map(mcp_prompt_message_text)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn mcp_prompt_message_text(message: &serde_json::Value) -> String {
    message
        .get("content")
        .filter(|content| content.get("type").and_then(serde_json::Value::as_str) == Some("text"))
        .and_then(|content| content.get("text"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn mcp_request_timeout(config: &serde_json::Value, client_name: &str) -> u64 {
    config
        .get("mcp")
        .and_then(serde_json::Value::as_object)
        .and_then(|servers| servers.get(client_name))
        .and_then(|server| server.get("timeout"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(oc_mcp::catalog::DEFAULT_REQUEST_TIMEOUT)
}

async fn command_registry(state: &AppState) -> Result<oc_command::command::Registry, ApiError> {
    let project_dir = FsPath::new(&state.location.directory);
    let mut registry = oc_command::command::Registry::new(project_dir);

    // Project command files conventionally live under `.opencode`, while
    // accepting a direct command/commands directory keeps compatibility with
    // callers that pass the command root itself.
    for directory in [project_dir.to_path_buf(), project_dir.join(".opencode")] {
        if let Ok(entries) = oc_command::command::load_from_dir(&directory) {
            registry.add_config_entries(entries);
        }
    }

    let config =
        crate::plugin_registry::merged_config(state, state.stores.read().await.config.clone());
    if let Some(commands) = config.get("command") {
        registry
            .add_config_commands(commands)
            .map_err(|error| ApiError::Unknown {
                message: format!("invalid command configuration: {error}"),
                reference: None,
            })?;
    }
    add_mcp_prompt_commands(state, &mut registry).await;
    let settings = skill_settings(project_dir, &config);
    if let Ok(service) = oc_command::skill::SkillService::load_with_environment(&settings) {
        let skills = service
            .available(None)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        registry.add_skills(&skills);
    }
    let plugin_skills = crate::plugin_registry::plugin_skill_infos(state);
    registry.add_skills(&plugin_skills);
    Ok(registry)
}

fn skill_settings(directory: &FsPath, config: &serde_json::Value) -> oc_command::skill::Settings {
    let paths = config
        .get("skills")
        .and_then(serde_json::Value::as_object)
        .and_then(|skills| skills.get("paths"))
        .and_then(serde_json::Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let home = oc_command::global::Global::detect().home;
    oc_command::skill::Settings {
        home,
        directory: directory.to_path_buf(),
        worktree: directory.to_path_buf(),
        disable_external_skills: false,
        disable_claude_code_skills: false,
        paths,
        pulled_dirs: Vec::new(),
        config_dirs: None,
    }
}

fn command_prompt_parts(body: &serde_json::Value, rendered: String) -> Vec<serde_json::Value> {
    let mut parts = body
        .get("parts")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    // The command text replaces any slash-command text supplied by a client,
    // but attachments remain part of the prompt.
    parts.retain(|part| part.get("type").and_then(|value| value.as_str()) != Some("text"));
    if parts.is_empty() {
        if let Some(files) = body.get("files").and_then(|value| value.as_array()) {
            parts.extend(files.iter().filter_map(|file| {
                let uri = file
                    .get("uri")
                    .or_else(|| file.get("url"))
                    .and_then(|value| value.as_str())?;
                let mut part = serde_json::json!({ "type": "file", "url": uri });
                if let Some(name) = file
                    .get("name")
                    .or_else(|| file.get("filename"))
                    .and_then(|value| value.as_str())
                {
                    part["filename"] = serde_json::Value::String(name.to_string());
                }
                Some(part)
            }));
        }
    }
    parts.push(serde_json::json!({
        "type": "text",
        "text": rendered,
        "synthetic": true,
    }));
    parts
}

fn run_command_shell(directory: &str, shell_command: &str) -> anyhow::Result<String> {
    #[cfg(windows)]
    let output = std::process::Command::new("cmd")
        .args(["/C", shell_command])
        .current_dir(directory)
        .output()?;

    #[cfg(not(windows))]
    let output = {
        let shell =
            std::env::var_os("SHELL").unwrap_or_else(|| std::ffi::OsString::from("/bin/sh"));
        std::process::Command::new(shell)
            .args(["-c", shell_command])
            .current_dir(directory)
            .output()?
    };

    if !output.status.success() {
        anyhow::bail!("shell command exited with {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// POST /session/:sessionID/shell.
pub async fn session_shell(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    session_prompt(State(state), Path(params), body).await
}

/// POST /session/:sessionID/revert. Stage a context boundary without deleting
/// the rolled-back messages from the durable/UI projection.
pub async fn session_revert(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let message_id = body
        .get("messageID")
        .or_else(|| body.get("messageId"))
        .and_then(|value| value.as_str())
        .ok_or(ApiError::V1BadRequest)?;
    let snapshot = body
        .get("snapshot")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    if !record
        .messages
        .iter()
        .any(|message| message.get("id").and_then(|value| value.as_str()) == Some(message_id))
    {
        return Err(ApiError::ApiNotFound {
            message: "Message not found".into(),
        });
    }
    let mut revert = serde_json::Map::new();
    revert.insert(
        "messageID".into(),
        serde_json::Value::String(message_id.into()),
    );
    if let Some(part_id) = body.get("partID").and_then(|value| value.as_str()) {
        revert.insert("partID".into(), serde_json::Value::String(part_id.into()));
    }
    if let Some(snapshot) = body.get("snapshot").and_then(|value| value.as_str()) {
        revert.insert(
            "snapshot".into(),
            serde_json::Value::String(snapshot.into()),
        );
    }
    record.info.revert = Some(serde_json::Value::Object(revert));
    record.info.time.updated = now_millis();
    let persisted_info = record.info.clone();
    let info = v1_info(&record.info, &state.location.directory);
    drop(stores);
    if let Some(snapshot) = snapshot {
        restore_project_snapshot(&state, &snapshot).await?;
    }
    state.persist_session(&persisted_info);
    json_value(info)
}

/// POST /session/:sessionID/unrevert.
pub async fn session_unrevert(
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
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    record.info.revert = None;
    record.info.time.updated = now_millis();
    let persisted_info = record.info.clone();
    let info = v1_info(&record.info, &state.location.directory);
    drop(stores);
    state.persist_session(&persisted_info);
    json_value(info)
}

/// GET /session/:sessionID/children.
pub async fn session_children(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let children = stores
        .sessions
        .values()
        .filter(|record| record.info.parent_id.as_deref() == Some(session_id.as_str()))
        .map(|record| v1_info(&record.info, &state.location.directory))
        .collect::<Vec<_>>();
    drop(stores);
    json_value(serde_json::Value::Array(children))
}

/// GET /session/:sessionID/todo.
pub async fn session_todo(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let todos = stores
        .todos
        .get(&session_id)
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    json_value(todos)
}

/// GET /session/:sessionID/diff.
pub async fn session_diff(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    _query: Query<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let directory = state
        .stores
        .read()
        .await
        .sessions
        .get(&session_id)
        .map(|record| record.info.location.directory.clone())
        .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
    let diff = git_output(&directory, &["diff", "--no-ext-diff", "--no-color"])
        .await
        .unwrap_or_default();
    json_value(parse_git_diff(&diff))
}

/// POST /session/:sessionID/fork.
pub async fn session_fork(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let source_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let (source, messages) = {
        let stores = state.stores.read().await;
        let source = stores
            .sessions
            .get(&source_id)
            .cloned()
            .ok_or_else(|| crate::errors::session_not_found(&source_id))?;
        let messages = if let Some(message_id) = body.get("messageID").and_then(|v| v.as_str()) {
            let end = source
                .messages
                .iter()
                .position(|message| message.get("id").and_then(|v| v.as_str()) == Some(message_id))
                .map(|index| index + 1)
                .unwrap_or(source.messages.len());
            source.messages[..end].to_vec()
        } else {
            source.messages.clone()
        };
        (source, messages)
    };
    let created = now_millis();
    let id = crate::event::session_id();
    let mut info = source.info;
    info.id = id.clone();
    info.parent_id = Some(source_id);
    info.time.created = created;
    info.time.updated = created;
    info.title = format!("{} (fork)", info.title);
    let record = crate::state::SessionRecord {
        info: info.clone(),
        messages: messages.clone(),
        active: false,
    };
    let mut stores = state.stores.write().await;
    stores.sessions.insert(id, record);
    drop(stores);
    state.persist_session(&info);
    for message in &messages {
        state.persist_message(&info.id, message);
    }
    json_value(v1_info(&info, &state.location.directory))
}

/// POST /session/:sessionID/share.
pub async fn session_share(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let config = state.stores.read().await.config.clone();
    if config.get("share").and_then(serde_json::Value::as_str) == Some("disabled") {
        return Err(ApiError::Unknown {
            message: "Sharing is disabled in configuration".into(),
            reference: None,
        });
    }

    let (info, messages) = {
        let stores = state.stores.read().await;
        let record = stores
            .sessions
            .get(&session_id)
            .ok_or_else(|| crate::errors::session_not_found(&session_id))?;
        (record.info.clone(), record.messages.clone())
    };
    let database = state.database.as_ref().ok_or_else(|| ApiError::Unknown {
        message: "Sharing requires a durable database".into(),
        reference: None,
    })?;
    let endpoint = crate::share::resolve(&config)
        .map_err(|error| share_api_error("configure", error))?
        .ok_or_else(|| ApiError::Unknown {
            message: "Sharing is disabled in configuration".into(),
            reference: None,
        })?;
    let client = reqwest::Client::new();
    let response = endpoint
        .apply_headers(client.post(endpoint.url(None, "")))
        .json(&serde_json::json!({ "sessionID": session_id }))
        .send()
        .await
        .map_err(|error| share_api_error("create", error))?
        .error_for_status()
        .map_err(|error| share_api_error("create", error))?;
    let share = response
        .json::<RemoteShare>()
        .await
        .map_err(|error| share_api_error("decode create response", error))?;

    let now = now_millis();
    let row = oc_database::tables::SessionShareRow {
        session_id: session_id.clone(),
        id: share.id.clone(),
        secret: share.secret.clone(),
        url: share.url.clone(),
        time_created: now,
        time_updated: now,
    };
    database
        .upsert(
            "session_share",
            &row,
            oc_database::tables::json_columns("session_share"),
            "session_id",
            &oc_database::Value::Text(session_id.clone()),
        )
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to persist share: {error}"),
            reference: None,
        })?;

    // The reference returns after creating the share and performs the initial
    // full sync in the background. Keep that ordering so a slow share service
    // does not make the local session-share operation appear to hang.
    let data = share_sync_data(&info, &messages);
    let sync_endpoint = endpoint.clone();
    tokio::spawn(async move {
        if let Err(error) = sync_remote_share(&sync_endpoint, &share, data).await {
            tracing::warn!(session_id = %session_id, ?error, "failed to sync shared session");
        }
    });

    json_value(serde_json::json!({ "url": row.url }))
}

/// DELETE /session/:sessionID/share.
pub async fn session_unshare(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let database = state.database.as_ref().ok_or_else(|| ApiError::Unknown {
        message: "Sharing requires a durable database".into(),
        reference: None,
    })?;
    let row = database
        .get_by::<oc_database::tables::SessionShareRow>(
            "session_share",
            "session_id",
            &oc_database::Value::Text(session_id.clone()),
            oc_database::tables::json_columns("session_share"),
        )
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to load share: {error}"),
            reference: None,
        })?;
    let Some(row) = row else {
        return no_content();
    };

    let config = state.stores.read().await.config.clone();
    let endpoint = crate::share::resolve(&config)
        .map_err(|error| share_api_error("configure", error))?
        .ok_or_else(|| ApiError::Unknown {
            message: "Sharing is disabled in configuration".into(),
            reference: None,
        })?;
    let mut request =
        endpoint.apply_headers(reqwest::Client::new().delete(endpoint.url(Some(&row.id), "")));
    if matches!(endpoint, crate::share::ShareEndpoint::Legacy { .. }) {
        request = request.json(&serde_json::json!({ "secret": row.secret }));
    }
    request
        .send()
        .await
        .map_err(|error| share_api_error("remove", error))?
        .error_for_status()
        .map_err(|error| share_api_error("remove", error))?;
    database
        .delete_by(
            "session_share",
            "session_id",
            &oc_database::Value::Text(session_id),
        )
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to remove persisted share: {error}"),
            reference: None,
        })?;
    no_content()
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct RemoteShare {
    id: String,
    url: String,
    #[serde(default)]
    secret: String,
}

fn share_api_error(operation: &str, error: impl std::fmt::Display) -> ApiError {
    ApiError::Unknown {
        message: format!("failed to {operation} shared session: {error}"),
        reference: None,
    }
}

fn share_sync_data(info: &SessionInfo, messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut data = vec![serde_json::json!({
        "type": "session",
        "data": info,
    })];
    for message in messages {
        data.push(serde_json::json!({
            "type": "message",
            "data": message,
        }));
        if let Some(parts) = message.get("parts").and_then(serde_json::Value::as_array) {
            data.extend(parts.iter().map(|part| {
                serde_json::json!({
                    "type": "part",
                    "data": part,
                })
            }));
        }
    }
    data
}

async fn sync_remote_share(
    endpoint: &crate::share::ShareEndpoint,
    share: &RemoteShare,
    data: Vec<serde_json::Value>,
) -> Result<(), reqwest::Error> {
    let mut body = serde_json::Map::new();
    if matches!(endpoint, crate::share::ShareEndpoint::Legacy { .. }) {
        body.insert(
            "secret".into(),
            serde_json::Value::String(share.secret.clone()),
        );
    }
    body.insert("data".into(), serde_json::Value::Array(data));
    endpoint
        .apply_headers(reqwest::Client::new().post(endpoint.url(Some(&share.id), "/sync")))
        .json(&serde_json::Value::Object(body))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// POST /session/:sessionID/init.
pub async fn session_init(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    _body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    json_value(serde_json::Value::Bool(true))
}

/// POST /session/:sessionID/summarize. Generate a provider-backed summary when
/// a session/request model is available, then persist a durable compaction
/// checkpoint while retaining the full message log for UI/export APIs.
pub async fn session_summarize(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    if !state.stores.read().await.sessions.contains_key(&session_id) {
        return Err(crate::errors::session_not_found(&session_id));
    }
    let body = body.0;
    let requested_model = body
        .get("model")
        .and_then(|model| {
            Some(ModelRef {
                id: model
                    .get("modelID")
                    .or_else(|| model.get("modelId"))
                    .or_else(|| model.get("id"))
                    .and_then(serde_json::Value::as_str)?
                    .to_string(),
                provider_id: model
                    .get("providerID")
                    .or_else(|| model.get("providerId"))
                    .and_then(serde_json::Value::as_str)?
                    .to_string(),
                variant: model
                    .get("variant")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .or_else(|| {
            Some(ModelRef {
                id: body
                    .get("modelID")
                    .or_else(|| body.get("modelId"))
                    .and_then(serde_json::Value::as_str)?
                    .to_string(),
                provider_id: body
                    .get("providerID")
                    .or_else(|| body.get("providerId"))
                    .and_then(serde_json::Value::as_str)?
                    .to_string(),
                variant: body
                    .get("variant")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        });
    let compacted = crate::runner::summarize_and_compact_session(
        &state,
        &session_id,
        oc_session_runner::session::message::CompactionReason::Manual,
        requested_model,
    )
    .await
    .map_err(|message| ApiError::Unknown {
        message: format!("session summary failed: {message}"),
        reference: None,
    })?;
    json_value(serde_json::Value::Bool(compacted))
}

/// POST /session/:sessionID/compact. TUI-compatible alias for manual
/// compaction; unlike the old placeholder it updates the runner context.
pub async fn session_compact(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    if !state.stores.read().await.sessions.contains_key(&session_id) {
        return Err(crate::errors::session_not_found(&session_id));
    }
    let _ = crate::runner::compact_session(
        &state,
        &session_id,
        oc_session_runner::session::message::CompactionReason::Manual,
    )
    .await;
    no_content()
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
    state.delete_message(&session_id, &message_id);
    json_value(serde_json::Value::Bool(true))
}

/// DELETE /session/:sessionID/message/:messageID/part/:partID.
pub async fn session_delete_part(
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
    let part_id = params
        .get("partID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    state.delete_part(&session_id, &message_id, &part_id);
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
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let request_exists = state
        .stores
        .read()
        .await
        .questions
        .contains_key(&request_id);
    if !request_exists {
        return Err(ApiError::QuestionNotFound {
            request_id: request_id.clone(),
            message: "Question request not found".into(),
        });
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
    state
        .question_service
        .reply(
            &oc_command::question::QuestionId::new(request_id.clone()),
            answers,
        )
        .map_err(|error| ApiError::QuestionNotFound {
            request_id: request_id.clone(),
            message: error.to_string(),
        })?;
    state.stores.write().await.questions.remove(&request_id);
    json_value(serde_json::Value::Bool(true))
}

/// POST /question/:requestID/reject.
pub async fn question_reject(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    if !state
        .stores
        .read()
        .await
        .questions
        .contains_key(&request_id)
    {
        return Err(ApiError::QuestionNotFound {
            request_id,
            message: "Question request not found".into(),
        });
    }
    state
        .question_service
        .reject(&oc_command::question::QuestionId::new(request_id.clone()))
        .map_err(|error| ApiError::QuestionNotFound {
            request_id: request_id.clone(),
            message: error.to_string(),
        })?;
    state.stores.write().await.questions.remove(&request_id);
    json_value(serde_json::Value::Bool(true))
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
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let resolved = state.resolve_permission(&request_id, &body.0).await;
    let mut stores = state.stores.write().await;
    if !resolved && stores.permissions.remove(&request_id).is_none() {
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

/// Resolve and persist the active project through the shared oc-project
/// service. The old implementation fabricated a project-shaped JSON value,
/// which meant `/project` never reflected git/VCS, names, icons, or sandboxes.
async fn current_project(state: &AppState) -> Result<oc_project::schema::ProjectInfo, ApiError> {
    state
        .project_runtime
        .project
        .from_directory(&state.location.directory)
        .await
        .map(|result| result.project)
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to resolve project: {error}"),
            reference: None,
        })
}

/// GET /project.
pub async fn project_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let current = current_project(&state).await?;
    let mut projects = state.project_runtime.project.list().await;
    if projects.is_empty() {
        projects.push(current);
    }
    json_value(serde_json::to_value(projects).unwrap_or_else(|_| serde_json::json!([])))
}

/// GET /project/current.
pub async fn project_current(State(state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::to_value(current_project(&state).await?).unwrap_or_default())
}

/// POST /project/git/init.
pub async fn project_git_init(State(state): State<crate::state::AppState>) -> HandlerResult {
    let project = current_project(&state).await?;
    let initialized = state
        .project_runtime
        .project
        .init_git(&state.location.directory, &project)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to initialize git repository: {error}"),
            reference: None,
        })?;
    json_value(serde_json::to_value(initialized).unwrap_or_default())
}

/// PATCH /project/:projectID.
pub async fn project_update(
    State(state): State<crate::state::AppState>,
    Path(path): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let project_id = path
        .get("projectID")
        .cloned()
        .unwrap_or_else(|| state.location.project_id.clone());
    let payload: oc_project::schema::ProjectUpdatePayload = serde_json::from_value(body.0)
        .map_err(|error| ApiError::InvalidRequest {
            message: format!("invalid project update: {error}"),
            kind: Some("project".into()),
            field: None,
        })?;
    let updated = state
        .project_runtime
        .project
        .update(&oc_project::schema::ProjectUpdateInput {
            projectID: oc_project::schema::ProjectID::make(project_id),
            name: payload.name,
            icon: payload.icon,
            commands: payload.commands,
        })
        .await
        .map_err(|error| ApiError::ApiNotFound {
            message: format!("project not found: {}", error.projectID),
        })?;
    json_value(serde_json::to_value(updated).unwrap_or_default())
}

/// GET /project/:projectID/directories.
pub async fn project_directories(
    State(state): State<crate::state::AppState>,
    Path(path): Path<HashMap<String, String>>,
) -> HandlerResult {
    let project_id = path
        .get("projectID")
        .cloned()
        .unwrap_or_else(|| state.location.project_id.clone());
    let project = state
        .project_runtime
        .project
        .get(&oc_project::schema::ProjectID::make(project_id.clone()))
        .await
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("project not found: {project_id}"),
        })?;
    let mut directories = vec![project.worktree];
    directories.extend(
        state
            .project_runtime
            .project
            .sandboxes(&oc_project::schema::ProjectID::make(project_id))
            .await,
    );
    directories.sort();
    directories.dedup();
    json_value(serde_json::json!({ "directories": directories }))
}

/// GET /provider.
pub async fn provider_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    let providers = crate::handlers::provider::provider_values_from_state_config(&state, &config);
    let connected = providers
        .iter()
        .filter_map(|provider| provider.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let default = providers
        .iter()
        .filter_map(|provider| {
            let provider_id = provider.get("id")?.as_str()?;
            let model_id = provider
                .get("models")?
                .as_object()?
                .iter()
                .find(|(_, model)| {
                    model.get("status").and_then(serde_json::Value::as_str) != Some("deprecated")
                })
                .map(|(model_id, _)| model_id.clone())?;
            Some((provider_id.to_string(), serde_json::Value::String(model_id)))
        })
        .collect::<serde_json::Map<_, _>>();
    json_value(serde_json::json!({
        "all": providers.clone(),
        "providers": providers,
        "default": default,
        "connected": connected,
        "authenticated": crate::handlers::provider::authenticated_provider_ids(),
        "preferred": [],
    }))
}

/// GET /provider/auth.
pub async fn provider_auth(State(state): State<crate::state::AppState>) -> HandlerResult {
    let methods = state.provider_auth.methods();
    json_value(
        serde_json::to_value(methods).map_err(|error| ApiError::Unknown {
            message: format!("failed to serialize provider auth methods: {error}"),
            reference: None,
        })?,
    )
}

/// POST /provider/:providerID/oauth/authorize.
pub async fn provider_oauth_authorize(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let provider_id = path
        .get("providerID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let input = serde_json::from_value::<oc_provider::provider::auth::AuthorizeInput>(body.0)
        .map_err(|_| ApiError::V1BadRequest)?;
    let authorization = state
        .provider_auth
        .authorize(&provider_id, &input)
        .map_err(|error| provider_auth_error(&provider_id, error))?;
    json_value(
        serde_json::to_value(authorization).map_err(|error| ApiError::Unknown {
            message: format!("failed to serialize provider authorization: {error}"),
            reference: None,
        })?,
    )
}

/// POST /provider/:providerID/oauth/callback.
pub async fn provider_oauth_callback(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let provider_id = path
        .get("providerID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let input = serde_json::from_value::<oc_provider::provider::auth::CallbackInput>(body.0)
        .map_err(|_| ApiError::V1BadRequest)?;
    let mut auth = oc_provider::auth::FileAuthStore::new(oc_mcp::auth::default_data_dir());
    state
        .provider_auth
        .callback(&provider_id, &input, &mut auth)
        .map_err(|error| provider_auth_error(&provider_id, error))?;
    json_value(serde_json::Value::Bool(true))
}

fn provider_auth_error(
    provider_id: &str,
    error: oc_provider::provider::auth::ProviderAuthError,
) -> ApiError {
    ApiError::ProviderAuth {
        provider_id: provider_id.to_string(),
        message: error.to_string(),
    }
}

/// GET /find. From the reference instance ripgrep handler.
pub async fn find_text(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let pattern = query
        .get("pattern")
        .or_else(|| query.get("query"))
        .cloned()
        .unwrap_or_default();
    if pattern.is_empty() {
        return json_value(serde_json::Value::Array(Vec::new()));
    }
    let file = query
        .get("path")
        .map(|path| {
            safe_workspace_path(&state.location.directory, path).ok_or_else(|| {
                ApiError::ApiNotFound {
                    message: "Not found".into(),
                }
            })
        })
        .transpose()?;
    let include = query.get("include").cloned();
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);
    let matches = oc_util::ripgrep::grep(oc_util::ripgrep::GrepInput {
        cwd: state.location.directory.clone(),
        pattern,
        file: file.map(|path| path.to_string_lossy().to_string()),
        include,
        limit,
        signal: None,
    })
    .await
    .unwrap_or_default();
    json_value(serde_json::to_value(matches).unwrap_or_else(|_| serde_json::json!([])))
}

/// GET /find/file.
pub async fn find_file(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let query_text = query.get("query").cloned().unwrap_or_default();
    let pattern = if query_text.is_empty() {
        "*".to_string()
    } else {
        format!("*{query_text}*")
    };
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);
    let entries = oc_util::ripgrep::find(oc_util::ripgrep::FindInput {
        cwd: state.location.directory.clone(),
        pattern,
        limit,
        hidden: false,
        follow: false,
        signal: None,
        on_entry: None,
    })
    .await
    .unwrap_or_default();
    json_value(serde_json::to_value(entries).unwrap_or_else(|_| serde_json::json!([])))
}

/// GET /find/symbol.
pub async fn find_symbol(
    State(state): State<crate::state::AppState>,
    query: Query<HashMap<String, String>>,
) -> HandlerResult {
    let symbol = query.get("query").cloned().unwrap_or_default();
    if symbol.is_empty() {
        return json_value(serde_json::Value::Array(Vec::new()));
    }
    let matches = oc_util::ripgrep::grep(oc_util::ripgrep::GrepInput {
        cwd: state.location.directory.clone(),
        pattern: format!(r"\b{symbol}\b"),
        file: None,
        include: query.get("include").cloned(),
        limit: query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(100),
        signal: None,
    })
    .await
    .unwrap_or_default();
    json_value(serde_json::to_value(matches).unwrap_or_else(|_| serde_json::json!([])))
}

/// GET /file. The legacy instance API is scoped to the active location.
pub async fn file_list(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let directory = state.location.directory.clone();
    let path = query.get("path").cloned().unwrap_or_default();
    let dir = safe_workspace_path(&directory, &path).ok_or_else(|| ApiError::ApiNotFound {
        message: "Not found".into(),
    })?;
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

/// GET /file/content. The legacy instance API is scoped to the active location.
pub async fn file_content(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let directory = state.location.directory.clone();
    let path = query.get("path").cloned().unwrap_or_default();
    let file = safe_workspace_path(&directory, &path).ok_or_else(|| ApiError::ApiNotFound {
        message: "Not found".into(),
    })?;
    let content = std::fs::read(&file).map_err(|_| ApiError::ApiNotFound {
        message: "Not found".into(),
    })?;
    json_value(serde_json::json!({
        "type": "text",
        "content": String::from_utf8_lossy(&content),
    }))
}

/// Resolve a user-supplied workspace-relative path without allowing lexical or
/// symlink traversal outside the selected workspace directory.
fn safe_workspace_path(directory: &str, path: &str) -> Option<PathBuf> {
    let relative = FsPath::new(path);
    if relative.is_absolute() {
        return None;
    }
    for component in relative.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return None;
        }
    }
    let base = std::fs::canonicalize(directory).ok()?;
    let candidate = base.join(relative);
    let canonical = std::fs::canonicalize(&candidate).ok()?;
    canonical.starts_with(&base).then_some(canonical)
}

/// Restore a project snapshot through the shared oc-project runtime. Snapshot
/// contexts are memoized by directory and disposed after this one-shot
/// operation, matching the reference instance lifecycle.
pub(crate) async fn restore_project_snapshot(
    state: &AppState,
    snapshot: &str,
) -> Result<(), ApiError> {
    let context = state
        .project_runtime
        .load(&state.location.directory)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to load project snapshot context: {error}"),
            reference: None,
        })?;
    state
        .project_runtime
        .snapshot
        .restore(&context, snapshot)
        .await;
    state.project_runtime.dispose(&context).await;
    Ok(())
}

/// GET /file/status. Reports working-tree state for files known to git.
pub async fn file_status(State(state): State<crate::state::AppState>) -> HandlerResult {
    let output = git_output(
        &state.location.directory,
        &["status", "--short", "--untracked-files=all"],
    )
    .await
    .unwrap_or_default();
    let rows = output
        .lines()
        .filter_map(|line| {
            let status = line.get(..2)?.trim().to_string();
            let path = line.get(3..)?.trim().to_string();
            (!path.is_empty()).then_some(serde_json::json!({
                "path": path,
                "status": status,
            }))
        })
        .collect::<Vec<_>>();
    json_value(serde_json::Value::Array(rows))
}

/// GET /mcp. Reports configured MCP servers and their local lifecycle state.
/// The actual transport is opened by `connect`; configuration remains durable
/// in the project config so a restarted server sees the same inventory.
pub async fn mcp_status(State(state): State<crate::state::AppState>) -> HandlerResult {
    let config = read_project_config(&state.location.directory);
    let connections = state.mcp_connections.lock().await.clone();
    let mut result = serde_json::Map::new();
    if let Some(servers) = config.get("mcp").and_then(serde_json::Value::as_object) {
        for (name, server) in servers {
            let enabled = server
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            let observed = connections.get(name);
            let status = observed
                .map(|connection| connection.status.as_str())
                .unwrap_or(if enabled { "disconnected" } else { "disabled" });
            let mut entry = serde_json::json!({
                "name": name,
                "status": status,
                "config": server,
            });
            if let Some(connection) = observed {
                if let Some(object) = entry.as_object_mut() {
                    object.insert("tools".into(), serde_json::json!(connection.tools));
                    if let Some(server_info) = &connection.server_info {
                        object.insert("serverInfo".into(), server_info.clone());
                    }
                    if let Some(error) = &connection.error {
                        object.insert("error".into(), serde_json::Value::String(error.clone()));
                    }
                }
            }
            result.insert(name.clone(), entry);
        }
    }
    json_value(serde_json::Value::Object(result))
}

/// POST /mcp.
pub async fn mcp_add(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let name = body
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or(ApiError::V1BadRequest)?;
    let server = body
        .get("config")
        .or_else(|| body.get("server"))
        .cloned()
        .or_else(|| {
            let mut value = body.0.clone();
            let object = value.as_object_mut()?;
            object.remove("name");
            Some(value)
        })
        .ok_or(ApiError::V1BadRequest)?;
    if !server.is_object() {
        return Err(ApiError::V1BadRequest);
    }
    let path = project_config_path(&state.location.directory);
    let mut config = read_project_config(&state.location.directory);
    let root = config.as_object_mut().ok_or(ApiError::V1BadRequest)?;
    let servers = root
        .entry("mcp")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or(ApiError::V1BadRequest)?;
    servers.insert(name.to_string(), server.clone());
    write_project_config(&path, &config).map_err(|error| ApiError::Unknown {
        message: error.to_string(),
        reference: None,
    })?;
    json_value(serde_json::json!({ "name": name, "config": server }))
}

fn project_config_path(directory: &str) -> PathBuf {
    let directory = FsPath::new(directory);
    let worktree = oc_config::paths::find_up(&[".git"], directory, None)
        .into_iter()
        .last()
        .and_then(|path| path.parent().map(PathBuf::from))
        .unwrap_or_else(|| directory.to_path_buf());
    oc_config::paths::files("opencode", directory, Some(&worktree))
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| worktree.join("opencode.json"))
}

fn read_project_config(directory: &str) -> serde_json::Value {
    let path = project_config_path(directory);
    let Ok(source) = std::fs::read_to_string(&path) else {
        return serde_json::json!({});
    };
    oc_config::parse::jsonc(&source, &path.display().to_string())
        .unwrap_or_else(|_| serde_json::json!({}))
}

fn write_project_config(path: &FsPath, config: &serde_json::Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_config_document(path, config)
}

fn write_config_document(path: &FsPath, config: &serde_json::Value) -> anyhow::Result<()> {
    if path.extension().and_then(|extension| extension.to_str()) == Some("jsonc") && path.exists() {
        if let Ok(source) = std::fs::read_to_string(path) {
            if let Ok(parsed) = oc_plugin::jsonc::parse(&source) {
                if let (Some(previous), Some(next)) = (parsed.value.as_object(), config.as_object())
                {
                    // Preserve the original document whenever the update is
                    // additive/replacement-only. Deletions fall back to a
                    // canonical rewrite because removing a value while
                    // retaining adjacent comments needs a full JSONC edit
                    // model rather than a value-span patch.
                    let can_patch = previous.keys().all(|key| next.contains_key(key));
                    if can_patch {
                        let mut patched = source;
                        for (key, value) in next {
                            let (_, next_text) =
                                oc_plugin::jsonc::patch_object_property(&patched, None, key, value)
                                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                            patched = next_text;
                        }
                        std::fs::write(path, patched)?;
                        return Ok(());
                    }
                }
            }
        }
    }
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(config)?))?;
    Ok(())
}

fn write_global_config(config: &serde_json::Value) -> anyhow::Result<()> {
    let directory = oc_config::paths::config_dir();
    std::fs::create_dir_all(&directory)?;
    let path = [
        directory.join("config.jsonc"),
        directory.join("config.json"),
    ]
    .into_iter()
    .find(|path| path.exists())
    .unwrap_or_else(|| directory.join("config.json"));
    write_config_document(&path, config)
}

/// POST /mcp/:name/auth.
pub async fn mcp_auth_start(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
) -> HandlerResult {
    let name = path.get("name").ok_or(ApiError::V1BadRequest)?;
    let result = state
        .mcp
        .start_auth(name)
        .await
        .map_err(|error| mcp_auth_error(name, error))?;
    json_value(serde_json::json!({
        "status": if result.authorization_url.is_empty() {
            "connected"
        } else {
            "authorization_required"
        },
        "authorizationUrl": if result.authorization_url.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(result.authorization_url)
        },
        "oauthState": result.oauth_state,
    }))
}

/// POST /mcp/:name/auth/callback.
pub async fn mcp_auth_callback(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let name = path.get("name").ok_or(ApiError::V1BadRequest)?;
    let code = body
        .get("code")
        .or_else(|| body.get("authorizationCode"))
        .and_then(serde_json::Value::as_str)
        .filter(|code| !code.is_empty())
        .ok_or(ApiError::V1BadRequest)?;
    let status = state
        .mcp
        .finish_auth(name, code)
        .await
        .map_err(|error| mcp_auth_error(name, error))?;
    json_value(
        serde_json::to_value(status).map_err(|error| ApiError::Unknown {
            message: format!("failed to serialize MCP auth status: {error}"),
            reference: None,
        })?,
    )
}

/// POST /mcp/:name/auth/authenticate.
pub async fn mcp_auth_authenticate(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
) -> HandlerResult {
    let name = path.get("name").ok_or(ApiError::V1BadRequest)?;
    let status = state
        .mcp
        .authenticate(name, None)
        .await
        .map_err(|error| mcp_auth_error(name, error))?;
    json_value(
        serde_json::to_value(status).map_err(|error| ApiError::Unknown {
            message: format!("failed to serialize MCP auth status: {error}"),
            reference: None,
        })?,
    )
}

/// DELETE /mcp/:name/auth.
pub async fn mcp_auth_remove(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
) -> HandlerResult {
    let name = path.get("name").ok_or(ApiError::V1BadRequest)?;
    state
        .mcp
        .remove_auth(name)
        .await
        .map_err(|error| mcp_auth_error(name, error))?;
    json_value(serde_json::json!({ "success": true }))
}

fn mcp_auth_error(name: &str, error: oc_mcp::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("MCP server not found") {
        ApiError::ApiNotFound { message }
    } else if error.is_unauthorized() {
        ApiError::Unauthorized { message }
    } else {
        ApiError::Unknown {
            message: format!("MCP OAuth for {name} failed: {message}"),
            reference: None,
        }
    }
}

/// POST /mcp/:name/connect.
/// Connect all enabled project MCP servers in the background during listener
/// startup. A failed server is retained as an error status so clients can
/// diagnose it through `/mcp` instead of seeing a silent disconnected entry.
pub(crate) async fn auto_connect_mcps(state: crate::state::AppState) {
    let config = read_project_config(&state.location.directory);
    let names = config
        .get("mcp")
        .and_then(serde_json::Value::as_object)
        .map(|servers| {
            servers
                .iter()
                .filter(|(_, value)| {
                    value
                        .get("enabled")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true)
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for name in names {
        let mut path = HashMap::new();
        path.insert("name".into(), name.clone());
        if let Err(error) = mcp_connect(State(state.clone()), Path(path)).await {
            state.mcp_connections.lock().await.insert(
                name,
                crate::state::McpConnection {
                    status: "error".into(),
                    error: Some(format!("{error:?}")),
                    ..Default::default()
                },
            );
        }
    }
}

async fn refresh_mcp_tools(
    state: crate::state::AppState,
    name: String,
    client: Arc<oc_mcp::client::Client>,
    timeout: u64,
) {
    let mut cursor = None;
    let mut tools = Vec::new();
    let mut native_tools = Vec::new();
    for _ in 0..100 {
        let page = match client.list_tools(cursor.clone(), timeout).await {
            Ok(page) => page,
            Err(error) => {
                tracing::debug!(server = %name, ?error, "MCP tools/list refresh failed");
                return;
            }
        };
        for tool in page.tools {
            native_tools.push(tool.clone());
            tools.push(serde_json::to_value(tool).unwrap_or(serde_json::Value::Null));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    let current = state.mcp_clients.lock().await.get(&name).cloned();
    if !current
        .as_ref()
        .map(|current| Arc::ptr_eq(current, &client))
        .unwrap_or(false)
    {
        return;
    }
    let mut runtime_tools = state.mcp_tools.lock().await;
    runtime_tools.retain(|_, tool| tool.server != name);
    for definition in native_tools {
        let key = oc_mcp::catalog::tool_name(&name, &definition.name);
        runtime_tools.insert(
            key,
            crate::state::McpRuntimeTool {
                server: name.clone(),
                definition,
                client: client.clone(),
                timeout,
            },
        );
    }
    drop(runtime_tools);
    if let Some(connection) = state.mcp_connections.lock().await.get_mut(&name) {
        connection.tools = tools;
    }
    state.emit_event(crate::event::Event {
        id: format!("mcp_tools_changed_{name}"),
        metadata: None,
        r#type: "mcp.tools.changed".into(),
        durable: None,
        location: None,
        data: serde_json::json!({ "server": name }),
    });
}

pub async fn mcp_connect(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
) -> HandlerResult {
    let name = path.get("name").ok_or(ApiError::V1BadRequest)?.to_string();
    let config = read_project_config(&state.location.directory);
    let raw = config
        .get("mcp")
        .and_then(|value| value.get(&name))
        .cloned()
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("MCP server {name} is not configured"),
        })?;
    let info: oc_mcp::config::Info =
        serde_json::from_value(raw).map_err(|error| ApiError::Unknown {
            message: format!("invalid MCP configuration for {name}: {error}"),
            reference: None,
        })?;
    if !info.enabled() {
        let connection = crate::state::McpConnection {
            status: "disabled".into(),
            ..Default::default()
        };
        state
            .mcp_connections
            .lock()
            .await
            .insert(name.clone(), connection);
        return json_value(serde_json::json!({ "name": name, "status": "disabled", "tools": [] }));
    }

    let timeout = info.timeout().unwrap_or(10_000);
    let transport: Arc<dyn oc_mcp::transport::Transport> = match info {
        oc_mcp::config::Info::Local(local) => {
            let mut command = local.command;
            let executable = command.drain(..1).next().ok_or_else(|| ApiError::Unknown {
                message: format!("MCP server {name} has no command"),
                reference: None,
            })?;
            let cwd = local
                .cwd
                .map(|path| FsPath::new(&state.location.directory).join(path))
                .unwrap_or_else(|| PathBuf::from(&state.location.directory));
            let mut environment = std::env::vars().collect::<Vec<_>>();
            if let Some(extra) = local.environment {
                for (key, value) in extra {
                    environment.retain(|(existing, _)| existing != &key);
                    environment.push((key, value));
                }
            }
            Arc::new(oc_mcp::transport::stdio::StdioTransport::new(
                executable,
                command,
                cwd,
                environment,
            ))
        }
        oc_mcp::config::Info::Remote(remote) => {
            let url = url::Url::parse(&remote.url).map_err(|error| ApiError::Unknown {
                message: format!("invalid MCP URL for {name}: {error}"),
                reference: None,
            })?;
            let auth_provider = if remote.oauth_enabled() {
                Some(Arc::new(oc_mcp::oauth_provider::McpOAuthProvider::new(
                    name.clone(),
                    remote.url.clone(),
                    oc_mcp::oauth_provider::McpOAuthConfig::from_config(remote.oauth_config()),
                    oc_mcp::oauth_provider::McpOAuthCallbacks::default(),
                    Arc::clone(&state.mcp_auth),
                ))
                    as Arc<dyn oc_mcp::oauth_provider::OAuthClientProvider>)
            } else {
                None
            };
            Arc::new(oc_mcp::transport::http::StreamableHTTPClientTransport::new(
                url,
                remote.headers,
                auth_provider,
            ))
        }
    };
    let client = oc_mcp::client::Client::connect(
        transport,
        oc_mcp::types::Implementation {
            name: "opencode-rs".into(),
            version: crate::version().into(),
        },
        oc_mcp::types::ClientCapabilities::default(),
        timeout,
    )
    .await
    .map_err(|error| ApiError::Unknown {
        message: format!("MCP connection failed for {name}: {error}"),
        reference: None,
    })?;
    let mut tools = Vec::new();
    let mut native_tools = Vec::new();
    let mut cursor = None;
    for _ in 0..100 {
        let page = match client.list_tools(cursor.clone(), timeout).await {
            Ok(page) => page,
            Err(error) => {
                let _ = client.close().await;
                return Err(ApiError::Unknown {
                    message: format!("MCP tools/list failed for {name}: {error}"),
                    reference: None,
                });
            }
        };
        for tool in page.tools {
            native_tools.push(tool.clone());
            tools.push(serde_json::to_value(tool).unwrap_or(serde_json::Value::Null));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let server_info = client
        .get_server_info()
        .await
        .and_then(|value| serde_json::to_value(value).ok());

    // A transport can terminate without an explicit `/mcp/:name/disconnect`
    // request (stdio child exit, SSE close, or a permanently failed HTTP
    // stream). Mirror the reference MCP watcher and evict only this exact
    // client so a late close from an older connection cannot remove a newer
    // replacement.
    let watched_state = state.clone();
    let watched_name = name.clone();
    let watched_client = client.clone();
    client
        .set_onclose(move || {
            let state = watched_state.clone();
            let name = watched_name.clone();
            let client = watched_client.clone();
            tokio::spawn(async move {
                let current = state.mcp_clients.lock().await.get(&name).cloned();
                if !current
                    .as_ref()
                    .map(|current| Arc::ptr_eq(current, &client))
                    .unwrap_or(false)
                {
                    return;
                }
                state.mcp_clients.lock().await.remove(&name);
                state
                    .mcp_tools
                    .lock()
                    .await
                    .retain(|_, tool| tool.server != name);
                let server_info = state
                    .mcp_connections
                    .lock()
                    .await
                    .get(&name)
                    .and_then(|connection| connection.server_info.clone());
                state.mcp_connections.lock().await.insert(
                    name.clone(),
                    crate::state::McpConnection {
                        status: "error".into(),
                        server_info,
                        tools: Vec::new(),
                        error: Some("Connection closed".into()),
                    },
                );
                state.emit_event(crate::event::Event {
                    id: format!("mcp_tools_changed_{name}"),
                    metadata: None,
                    r#type: "mcp.tools.changed".into(),
                    durable: None,
                    location: None,
                    data: serde_json::json!({ "server": name }),
                });
            });
        })
        .await;

    if client
        .get_server_capabilities()
        .await
        .map(|capabilities| capabilities.has_tools())
        .unwrap_or(false)
    {
        let state = state.clone();
        let name = name.clone();
        let watched_client = client.clone();
        client
            .set_notification_handler(
                "notifications/tools/list_changed",
                Arc::new(move |_params: Option<serde_json::Value>| {
                    let state = state.clone();
                    let name = name.clone();
                    let client = watched_client.clone();
                    tokio::spawn(refresh_mcp_tools(state, name, client, timeout));
                }),
            )
            .await;
    }
    if let Some(previous) = state
        .mcp_clients
        .lock()
        .await
        .insert(name.clone(), client.clone())
    {
        let _ = previous.close().await;
    }
    let prefix = format!("{}_", oc_mcp::catalog::sanitize(&name));
    let mut runtime_tools = state.mcp_tools.lock().await;
    runtime_tools.retain(|key, _| !key.starts_with(&prefix));
    for definition in native_tools {
        let key = oc_mcp::catalog::tool_name(&name, &definition.name);
        runtime_tools.insert(
            key,
            crate::state::McpRuntimeTool {
                server: name.clone(),
                definition,
                client: client.clone(),
                timeout,
            },
        );
    }
    drop(runtime_tools);
    state.mcp_connections.lock().await.insert(
        name.clone(),
        crate::state::McpConnection {
            status: "connected".into(),
            server_info: server_info.clone(),
            tools: tools.clone(),
            error: None,
        },
    );
    json_value(serde_json::json!({
        "name": name,
        "status": "connected",
        "serverInfo": server_info,
        "tools": tools,
    }))
}

/// POST /mcp/:name/disconnect.
pub async fn mcp_disconnect(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
) -> HandlerResult {
    let name = path.get("name").ok_or(ApiError::V1BadRequest)?.to_string();
    if let Some(client) = state.mcp_clients.lock().await.remove(&name) {
        let _ = client.close().await;
    }
    state
        .mcp_tools
        .lock()
        .await
        .retain(|_, tool| tool.server != name);
    state.mcp_connections.lock().await.remove(&name);
    json_value(serde_json::json!({ "name": name, "status": "disconnected" }))
}

/// POST /sync/start.
pub async fn sync_start(State(_state): State<crate::state::AppState>) -> HandlerResult {
    oc_sync::sync::store::register_session_durable_definitions();
    json_value(serde_json::Value::Bool(true))
}

/// POST /sync/replay.
pub async fn sync_replay(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let payload: oc_sync::control_plane::sync_api::ReplayPayload =
        serde_json::from_value(body.0).map_err(|error| ApiError::InvalidRequest {
            message: format!("invalid sync replay payload: {error}"),
            kind: Some("sync.replay".into()),
            field: None,
        })?;
    let session_id = payload
        .events
        .first()
        .map(|event| event.aggregate_id.clone())
        .unwrap_or_default();
    for event in payload.events {
        state
            .sync_store
            .replay(
                &event,
                &oc_sync::sync::store::ReplayOptions {
                    publish: true,
                    owner_id: Some(state.location.directory.clone()),
                    strict_owner: false,
                },
            )
            .map_err(|error| ApiError::InvalidRequest {
                message: format!("sync replay failed: {error}"),
                kind: Some("sync.replay".into()),
                field: None,
            })?;
    }
    json_value(serde_json::json!({ "sessionID": session_id }))
}

/// POST /sync/steal.
pub async fn sync_steal(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = body
        .0
        .get("sessionID")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(ApiError::V1BadRequest)?;
    state
        .sync_store
        .claim(session_id, &state.location.directory);
    json_value(serde_json::json!({ "sessionID": session_id }))
}

/// POST /sync/history.
pub async fn sync_history(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let cursors = body.0.as_object().ok_or(ApiError::V1BadRequest)?;
    let mut history: Vec<serde_json::Value> = Vec::new();
    for (aggregate_id, cursor) in cursors {
        let after = cursor
            .as_i64()
            .or_else(|| cursor.as_u64().and_then(|value| i64::try_from(value).ok()))
            .ok_or(ApiError::V1BadRequest)?;
        let events = state
            .sync_store
            .read_after(aggregate_id, after)
            .map_err(|error| ApiError::Unknown {
                message: format!("sync history failed: {error}"),
                reference: None,
            })?;
        history.extend(events.into_iter().filter_map(|event| {
            let durable = event.durable?;
            Some(serde_json::json!({
                "id": event.id,
                "aggregate_id": durable.aggregate_id,
                "seq": durable.seq,
                "type": format!("{}.{}", event.r#type, durable.version),
                "data": event.data,
            }))
        }));
    }
    history.sort_by(|left, right| {
        left.get("seq")
            .and_then(serde_json::Value::as_i64)
            .cmp(&right.get("seq").and_then(serde_json::Value::as_i64))
    });
    json_value(serde_json::Value::Array(history))
}

/// POST /tui/*. These publish TUI control events for an attached TUI client.
pub async fn tui_open_help(State(_state): State<crate::state::AppState>) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/open-help".into(),
        body: serde_json::json!({}),
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_open_sessions(State(_state): State<crate::state::AppState>) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/open-sessions".into(),
        body: serde_json::json!({}),
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_open_themes(State(_state): State<crate::state::AppState>) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/open-themes".into(),
        body: serde_json::json!({}),
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_open_models(State(_state): State<crate::state::AppState>) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/open-models".into(),
        body: serde_json::json!({}),
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_submit_prompt(State(_state): State<crate::state::AppState>) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/submit-prompt".into(),
        body: serde_json::json!({}),
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_clear_prompt(State(_state): State<crate::state::AppState>) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/clear-prompt".into(),
        body: serde_json::json!({}),
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_append_prompt(
    State(_state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/append-prompt".into(),
        body: body.0,
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_execute_command(
    State(_state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/execute-command".into(),
        body: body.0,
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_show_toast(
    State(_state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/show-toast".into(),
        body: body.0,
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_publish(
    State(_state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/publish".into(),
        body: body.0,
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_select_session(
    State(_state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    crate::shared::tui_control::submit_tui_request(crate::shared::tui_control::TuiRequest {
        path: "/tui/select-session".into(),
        body: body.0,
    });
    json_value(serde_json::Value::Bool(true))
}

pub async fn tui_control_next(State(_state): State<crate::state::AppState>) -> HandlerResult {
    let request =
        crate::shared::tui_control::next_tui_request()
            .await
            .ok_or(ApiError::Unknown {
                message: "TUI control queue is closed".into(),
                reference: None,
            })?;
    json_value(serde_json::json!({
        "path": request.path,
        "body": request.body,
    }))
}

pub async fn tui_control_response(
    State(_state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    crate::shared::tui_control::submit_tui_response(body.0);
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
    query: Query<HashMap<String, String>>,
) -> HandlerResult {
    let registry = oc_tool::core::registry_with_builtins(false, false);
    let definitions = registry
        .materialize(&[])
        .definitions
        .into_iter()
        .filter(|definition| {
            query
                .get("name")
                .map(|name| name == &definition.name)
                .unwrap_or(true)
        })
        .map(|definition| serde_json::to_value(definition).unwrap_or(serde_json::Value::Null))
        .collect::<Vec<_>>();
    json_value(serde_json::Value::Array(definitions))
}

/// GET /experimental/tool/ids.
pub async fn experimental_tool_ids(State(_state): State<crate::state::AppState>) -> HandlerResult {
    let registry = oc_tool::core::registry_with_builtins(false, false);
    let names = registry
        .materialize(&[])
        .definitions
        .into_iter()
        .map(|definition| serde_json::Value::String(definition.name))
        .collect::<Vec<_>>();
    json_value(serde_json::Value::Array(names))
}

/// GET /experimental/worktree.
pub async fn experimental_worktree_list(
    State(state): State<crate::state::AppState>,
) -> HandlerResult {
    let context = state
        .project_runtime
        .load(&state.location.directory)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to load project for worktree listing: {error}"),
            reference: None,
        })?;
    let result = state
        .project_runtime
        .worktree
        .list(&context)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("{}: {}", error.tag(), error.message()),
            reference: None,
        })?;
    json_value(
        serde_json::to_value(result).map_err(|error| ApiError::Unknown {
            message: format!("failed to encode worktree list: {error}"),
            reference: None,
        })?,
    )
}

/// POST /experimental/worktree.
pub async fn experimental_worktree_create(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let input: oc_project::schema::WorktreeCreateInput =
        serde_json::from_value(body.0).map_err(|error| ApiError::InvalidRequest {
            message: format!("invalid worktree create input: {error}"),
            kind: Some("WorktreeCreateInput".into()),
            field: None,
        })?;
    let context = state
        .project_runtime
        .load(&state.location.directory)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to load project for worktree creation: {error}"),
            reference: None,
        })?;
    let result = state
        .project_runtime
        .worktree
        .create(&context, Some(&input))
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("{}: {}", error.tag(), error.message()),
            reference: None,
        })?;
    json_value(
        serde_json::to_value(result).map_err(|error| ApiError::Unknown {
            message: format!("failed to encode created worktree: {error}"),
            reference: None,
        })?,
    )
}

/// DELETE /experimental/worktree.
pub async fn experimental_worktree_remove(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let input: oc_project::schema::WorktreeRemoveInput =
        serde_json::from_value(body.0).map_err(|error| ApiError::InvalidRequest {
            message: format!("invalid worktree remove input: {error}"),
            kind: Some("WorktreeRemoveInput".into()),
            field: None,
        })?;
    let context = state
        .project_runtime
        .load(&state.location.directory)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to load project for worktree removal: {error}"),
            reference: None,
        })?;
    let result = state
        .project_runtime
        .worktree
        .remove(&context, &input)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("{}: {}", error.tag(), error.message()),
            reference: None,
        })?;
    json_value(serde_json::Value::Bool(result))
}

/// POST /experimental/worktree/reset.
pub async fn experimental_worktree_reset(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let input: oc_project::schema::WorktreeResetInput =
        serde_json::from_value(body.0).map_err(|error| ApiError::InvalidRequest {
            message: format!("invalid worktree reset input: {error}"),
            kind: Some("WorktreeResetInput".into()),
            field: None,
        })?;
    let context = state
        .project_runtime
        .load(&state.location.directory)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to load project for worktree reset: {error}"),
            reference: None,
        })?;
    let result = state
        .project_runtime
        .worktree
        .reset(&context, &input)
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("{}: {}", error.tag(), error.message()),
            reference: None,
        })?;
    json_value(serde_json::Value::Bool(result))
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

/// GET /experimental/session/background.
///
/// This is a bounded observation surface for the process-local background
/// registry. It intentionally does not claim durable or cross-process state.
pub async fn experimental_session_background_list(
    State(state): State<crate::state::AppState>,
) -> HandlerResult {
    let jobs = state.background_jobs.list().await;
    json_value(serde_json::to_value(jobs)?)
}

/// GET /experimental/session/:sessionID/background.
pub async fn experimental_session_background_status(
    State(state): State<crate::state::AppState>,
    Path(path): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = path
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let info = state
        .background_jobs
        .get(&session_id)
        .await
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("Background job `{session_id}` not found"),
        })?;
    json_value(serde_json::to_value(info)?)
}

/// DELETE /experimental/session/:sessionID/background.
pub async fn experimental_session_background_cancel(
    State(state): State<crate::state::AppState>,
    Path(path): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = path
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let info = state
        .background_jobs
        .cancel(&session_id)
        .await
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("Background job `{session_id}` not found"),
        })?;
    // Background subagents also register a cooperative runner token. The
    // registry remains the source of truth for job status; this stops the
    // associated provider/tool turn when one is present.
    state.cancel_session_run(&session_id).await;
    json_value(serde_json::to_value(info)?)
}

/// POST /experimental/session/:sessionID/background.
pub async fn experimental_session_background(
    State(state): State<crate::state::AppState>,
    Path(path): Path<HashMap<String, String>>,
) -> HandlerResult {
    if let Some(session_id) = path.get("sessionID") {
        // Preserve the reference boolean response while making this route
        // useful for an already-running registered job.
        let _ = state.background_jobs.promote(session_id).await;
    }
    json_value(serde_json::Value::Bool(true))
}

/// GET /experimental/resource.
pub async fn experimental_resource(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json_value(serde_json::json!({}))
}

/// GET /experimental/workspace/adapter.
pub async fn workspace_adapters(State(state): State<crate::state::AppState>) -> HandlerResult {
    let entries = oc_sync::control_plane::adapters::list_adapters(&state.location.project_id);
    json_value(serde_json::to_value(entries)?)
}

/// GET /experimental/workspace.
pub async fn workspace_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let project_id = state.location.project_id.clone();
    let mut workspaces: Vec<_> = state
        .workspaces
        .lock()
        .await
        .values()
        .filter(|workspace| {
            workspace
                .get("projectID")
                .and_then(serde_json::Value::as_str)
                == Some(project_id.as_str())
        })
        .cloned()
        .collect();
    workspaces.sort_by(|left, right| {
        left.get("id")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("id").and_then(serde_json::Value::as_str))
    });
    json_value(serde_json::Value::Array(workspaces))
}

/// POST /experimental/workspace.
pub async fn workspace_create(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let input = body.0;
    let name = input
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(ApiError::V1BadRequest)?;
    let project_id = input
        .get("projectID")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&state.location.project_id)
        .to_string();
    let id = input
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            oc_sync::sync::schema::ascending(oc_sync::sync::schema::Prefix::Workspace, None)
                .expect("workspace id")
        });
    let workspace = serde_json::json!({
        "id": id,
        "type": input.get("type").and_then(serde_json::Value::as_str).unwrap_or("worktree"),
        "name": name,
        "branch": input.get("branch").cloned().unwrap_or(serde_json::Value::Null),
        "directory": input.get("directory").cloned().unwrap_or(serde_json::Value::Null),
        "extra": input.get("extra").cloned().unwrap_or(serde_json::Value::Null),
        "projectID": project_id,
        "timeUsed": now_millis(),
    });
    let id = workspace["id"].as_str().unwrap_or_default().to_string();
    state.workspaces.lock().await.insert(id, workspace.clone());
    json_value(workspace)
}

/// POST /experimental/workspace/sync-list.
pub async fn workspace_sync_list(State(state): State<crate::state::AppState>) -> HandlerResult {
    let project_id = state.location.project_id.clone();
    let known = {
        let workspaces = state.workspaces.lock().await;
        workspaces
            .values()
            .filter(|workspace| {
                workspace
                    .get("projectID")
                    .and_then(serde_json::Value::as_str)
                    == Some(project_id.as_str())
            })
            .filter_map(|workspace| {
                serde_json::from_value::<oc_sync::control_plane::types::WorkspaceInfo>(
                    workspace.clone(),
                )
                .ok()
            })
            .collect::<Vec<_>>()
    };
    let api: Arc<dyn oc_sync::control_plane::sync_api::SyncApi> =
        Arc::new(oc_sync::control_plane::sync_api::ReqwestSyncApi::default());

    // Keep discovery scoped to this instance's project. Builtin adapters that
    // are unavailable in the current environment are best-effort, matching
    // the reference syncList behavior; successful adapter results are still
    // projected into the server's HTTP workspace store below.
    let discovered = oc_sync::control_plane::workspace_context::InstanceContext::provide(
        Some(project_id.clone()),
        async {
            let mut discovered = Vec::new();
            for (ty, adapter) in oc_sync::control_plane::adapters::registered_adapters(&project_id)
            {
                let target = match known.iter().find(|workspace| workspace.ty == ty) {
                    Some(workspace) => {
                        // Resolve the target only when a workspace of this
                        // adapter type already supplies one; first-use remote
                        // discovery is intentionally still best-effort.
                        oc_sync::control_plane::workspace_adapter_runtime::target(workspace)
                            .await
                            .ok()
                    }
                    None => None,
                };
                match oc_sync::control_plane::workspace_adapter_runtime::list_with_api(
                    &adapter,
                    api.clone(),
                    target,
                )
                .await
                {
                    Ok(items) => discovered.extend(items),
                    Err(error) => {
                        tracing::debug!(adapter = %ty, %error, "workspace adapter discovery unavailable")
                    }
                }
            }
            discovered
        },
    )
    .await;

    let mut workspaces = state.workspaces.lock().await;
    let mut names = workspaces
        .values()
        .filter(|workspace| {
            workspace
                .get("projectID")
                .and_then(serde_json::Value::as_str)
                == Some(project_id.as_str())
        })
        .filter_map(|workspace| workspace.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    for item in discovered {
        if item.project_id != project_id || !names.insert(item.name.clone()) {
            continue;
        }
        let id = oc_sync::sync::schema::ascending(oc_sync::sync::schema::Prefix::Workspace, None)
            .map_err(|error| ApiError::Unknown {
            message: format!("failed to allocate workspace id: {error}"),
            reference: None,
        })?;
        let mut value = serde_json::to_value(item)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("id".into(), serde_json::Value::String(id.clone()));
            object.insert("timeUsed".into(), serde_json::Value::from(now_millis()));
        }
        workspaces.insert(id, value);
    }
    no_content()
}

/// GET /experimental/workspace/status.
pub async fn workspace_status(State(state): State<crate::state::AppState>) -> HandlerResult {
    let project_id = state.location.project_id.clone();
    let workspaces = state.workspaces.lock().await;
    let statuses = workspaces
        .values()
        .filter(|workspace| {
            workspace
                .get("projectID")
                .and_then(serde_json::Value::as_str)
                == Some(project_id.as_str())
        })
        .map(|workspace| {
            serde_json::json!({
                "workspaceID": workspace.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "status": "ready"
            })
        })
        .collect::<Vec<_>>();
    json_value(serde_json::Value::Array(statuses))
}

/// DELETE /experimental/workspace/:id.
pub async fn workspace_remove(
    State(state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
) -> HandlerResult {
    let id = path.get("id").ok_or(ApiError::V1BadRequest)?;
    let removed = state.workspaces.lock().await.remove(id);
    if removed.is_none() {
        return Err(ApiError::ApiNotFound {
            message: format!("Workspace not found: {id}"),
        });
    }
    json_value(serde_json::Value::Bool(true))
}

/// POST /experimental/workspace/warp.
pub async fn workspace_warp(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = body
        .get("sessionID")
        .and_then(serde_json::Value::as_str)
        .ok_or(ApiError::V1BadRequest)?;
    let workspace_id = body.get("workspaceID").and_then(serde_json::Value::as_str);
    let directory = if let Some(workspace_id) = workspace_id {
        state
            .workspaces
            .lock()
            .await
            .get(workspace_id)
            .and_then(|workspace| workspace.get("directory"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ApiError::ApiNotFound {
                message: format!("Workspace not found: {workspace_id}"),
            })?
    } else {
        state.location.directory.clone()
    };
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(session_id)
        .ok_or_else(|| ApiError::SessionNotFound {
            session_id: session_id.to_string(),
            message: "Session not found".into(),
        })?;
    record.info.location.directory = directory;
    record.info.location.workspace_id = workspace_id.map(str::to_string);
    let info = record.info.clone();
    drop(stores);
    state.persist_session(&info);
    state.emit_event(crate::event::Event {
        id: crate::event::event_id(),
        metadata: None,
        r#type: "session.updated".into(),
        durable: None,
        location: Some(state.location.reference()),
        data: serde_json::json!({ "sessionID": session_id, "workspaceID": workspace_id }),
    });
    no_content()
}

/// POST /experimental/control-plane/move-session.
pub async fn control_plane_move_session(
    State(state): State<crate::state::AppState>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = body
        .get("sessionID")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(ApiError::V1BadRequest)?;
    let workspace_id = body
        .get("workspaceID")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(ApiError::V1BadRequest)?;
    let workspace = state
        .workspaces
        .lock()
        .await
        .get(workspace_id)
        .cloned()
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("Workspace not found: {workspace_id}"),
        })?;
    let directory = workspace
        .get("directory")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::InvalidRequest {
            message: format!("Workspace `{workspace_id}` has no local directory"),
            kind: Some("workspace.move".into()),
            field: Some("directory".into()),
        })?;
    let mut stores = state.stores.write().await;
    let record = stores
        .sessions
        .get_mut(session_id)
        .ok_or_else(|| ApiError::SessionNotFound {
            session_id: session_id.to_string(),
            message: "Session not found".into(),
        })?;
    record.info.location.directory = directory.to_string();
    record.info.location.workspace_id = Some(workspace_id.to_string());
    let info = record.info.clone();
    drop(stores);
    state.persist_session(&info);
    state.emit_event(crate::event::Event {
        id: crate::event::event_id(),
        metadata: None,
        r#type: "session.updated".into(),
        durable: None,
        location: Some(state.location.reference()),
        data: serde_json::json!({ "sessionID": session_id, "workspaceID": workspace_id }),
    });
    no_content()
}

/// PUT /auth/:providerID.
pub async fn control_auth_set(
    State(_state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    use oc_provider::auth::{Api, AuthStore, FileAuthStore, Info, Oauth, WellKnown};

    let provider_id = path
        .get("providerID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let value = body.0;
    let info = if value.get("type").is_some() {
        serde_json::from_value::<Info>(value).map_err(|error| ApiError::InvalidRequest {
            message: format!("invalid provider credential: {error}"),
            kind: Some("credential".into()),
            field: None,
        })?
    } else if let Some(key) = value.get("key").and_then(|value| value.as_str()) {
        Info::Api(Api {
            key: key.to_string(),
            metadata: value
                .get("metadata")
                .and_then(|metadata| serde_json::from_value(metadata.clone()).ok()),
        })
    } else if value.get("access").is_some() || value.get("refresh").is_some() {
        Info::Oauth(Oauth {
            refresh: value
                .get("refresh")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            access: value
                .get("access")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            expires: value
                .get("expires")
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            account_id: value
                .get("accountID")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned),
            enterprise_url: value
                .get("enterpriseURL")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned),
        })
    } else if let Some(token) = value.get("token").and_then(|value| value.as_str()) {
        Info::WellKnown(WellKnown {
            key: value
                .get("key")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            token: token.to_string(),
        })
    } else {
        return Err(ApiError::InvalidRequest {
            message: "credential must contain key, access/refresh, token, or type".into(),
            kind: Some("credential".into()),
            field: None,
        });
    };

    let mut store = FileAuthStore::new(oc_mcp::auth::default_data_dir());
    store
        .set(&provider_id, info)
        .map_err(|error| ApiError::Unknown {
            message: error.to_string(),
            reference: None,
        })?;
    json_value(serde_json::Value::Bool(true))
}

/// DELETE /auth/:providerID.
pub async fn control_auth_remove(
    State(_state): State<crate::state::AppState>,
    path: Path<HashMap<String, String>>,
) -> HandlerResult {
    use oc_provider::auth::{AuthStore, FileAuthStore};

    let provider_id = path
        .get("providerID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let mut store = FileAuthStore::new(oc_mcp::auth::default_data_dir());
    store
        .remove(&provider_id)
        .map_err(|error| ApiError::Unknown {
            message: error.to_string(),
            reference: None,
        })?;
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

#[cfg(test)]
mod tests {
    use super::{
        command_prompt_parts, configured_model_for_config_with_auth_and_base, limit_messages,
        mcp_prompt_message_text, model_from_value, prompt_text, run_command_shell,
        safe_workspace_path,
    };

    #[test]
    fn prompt_text_accepts_v1_parts_and_shorthand() {
        let parts = serde_json::json!({
            "parts": [
                {"type": "text", "text": "first"},
                {"type": "file", "url": "file:///tmp/a.txt"},
                {"type": "text", "text": "second"}
            ]
        });
        assert_eq!(prompt_text(&parts), "first\nsecond");
        assert_eq!(
            prompt_text(&serde_json::json!({"prompt": "hello"})),
            "hello"
        );
        assert_eq!(
            prompt_text(&serde_json::json!({"prompt": {"text": "nested"}})),
            "nested"
        );
    }

    #[test]
    fn message_limit_keeps_newest_context_in_order() {
        let mut messages = vec![
            serde_json::json!({"id": "old"}),
            serde_json::json!({"id": "middle"}),
            serde_json::json!({"id": "new"}),
        ];
        limit_messages(&mut messages, Some(2));
        assert_eq!(
            messages
                .iter()
                .map(|message| message["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["middle", "new"]
        );
    }

    #[test]
    fn workspace_path_rejects_escape_components() {
        assert!(safe_workspace_path("/definitely/missing", "../secret").is_none());
        assert!(safe_workspace_path("/definitely/missing", "/etc/passwd").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn command_template_renders_arguments_and_shell_expansion_into_parts() {
        let mut registry = oc_command::command::Registry::new("/tmp");
        registry
            .add_config_commands(&serde_json::json!({
                "fix": { "template": "Fix $1; args=$ARGUMENTS; shell=!`printf shell-ok`" }
            }))
            .expect("valid command");
        let command = registry.get("fix").expect("registered command");
        let rendered = command.render("\"first file\" second");
        let expanded =
            oc_command::command::expand_shell(&rendered, &|shell| run_command_shell("/tmp", shell))
                .expect("expand shell")
                .trim()
                .to_string();

        assert_eq!(
            expanded,
            "Fix first file second; args=\"first file\" second; shell=shell-ok"
        );
        let parts = command_prompt_parts(
            &serde_json::json!({
                "parts": [
                    {"type": "file", "url": "file:///tmp/a.txt"},
                    {"type": "text", "text": "/fix"}
                ]
            }),
            expanded,
        );
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "file");
        assert_eq!(
            parts[1]["text"],
            "Fix first file second; args=\"first file\" second; shell=shell-ok"
        );
        assert_eq!(
            model_from_value(&serde_json::json!("stub/demo")).provider_id,
            "stub"
        );
    }

    #[test]
    fn mcp_prompt_messages_join_text_and_ignore_non_text_content() {
        let messages = vec![
            serde_json::json!({
                "role": "user",
                "content": {"type": "text", "text": "first"}
            }),
            serde_json::json!({
                "role": "assistant",
                "content": {"type": "image", "data": "..."}
            }),
            serde_json::json!({
                "role": "user",
                "content": {"type": "text", "text": "last"}
            }),
        ];
        let text = messages
            .iter()
            .map(mcp_prompt_message_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text, "first\n\nlast");
    }

    #[test]
    fn configured_model_supports_all_embedded_provider_facades() {
        for (provider, model) in [
            ("azure", "gpt-4o"),
            ("cloudflare-ai-gateway", "@cf/meta/llama-3.1-8b-instruct"),
            ("cloudflare-workers-ai", "@cf/meta/llama-3.1-8b-instruct"),
            ("github-copilot", "gpt-4o"),
            ("amazon-bedrock", "anthropic.claude-3-sonnet"),
        ] {
            assert!(
                super::configured_model(provider, model).is_ok(),
                "provider facade should resolve: {provider}"
            );
        }
    }

    #[test]
    fn native_openai_oauth_uses_codex_responses_endpoint() {
        let auth = oc_llm::route::auth::Auth::Credential {
            credential: oc_llm::route::auth::Credential::Value("oauth-token".into()),
            render: oc_llm::route::auth::HeaderRender::Bearer,
        };
        let model = configured_model_for_config_with_auth_and_base(
            &serde_json::json!({}),
            "openai",
            "gpt-5.3-codex",
            Some(auth),
            Some("https://chatgpt.com/backend-api/codex"),
        )
        .expect("native Codex route should resolve");

        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some("https://chatgpt.com/backend-api/codex")
        );
        assert!(matches!(
            &model.route.endpoint.path,
            oc_llm::route::endpoint::EndpointPath::Static(path) if path == "/responses"
        ));
    }

    #[test]
    fn native_github_copilot_oauth_uses_enterprise_endpoint() {
        let auth = oc_llm::route::auth::Auth::Credential {
            credential: oc_llm::route::auth::Credential::Value("oauth-token".into()),
            render: oc_llm::route::auth::HeaderRender::Bearer,
        };
        let model = configured_model_for_config_with_auth_and_base(
            &serde_json::json!({}),
            "github-copilot",
            "gpt-4o",
            Some(auth),
            Some("https://copilot-api.company.ghe.com"),
        )
        .expect("native Copilot route should resolve");
        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some("https://copilot-api.company.ghe.com")
        );
    }
}
