//! Filesystem handler. From reference/packages/server/src/handlers/fs.ts.

use std::path::PathBuf;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;

use super::{json, request_location, HandlerResult};
use crate::errors::ApiError;
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `fs.read` — serve one file relative to the requested location with its mime type.
/// From reference/packages/server/src/handlers/fs.ts.
pub async fn fs_read(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    path: axum::extract::Path<String>,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let relative = safe_read_path(&path).ok_or_else(|| ApiError::ApiNotFound {
        message: "Not Found".into(),
    })?;
    let base = std::fs::canonicalize(&location.directory).map_err(|_| ApiError::ApiNotFound {
        message: "Not Found".into(),
    })?;
    let file_path = base.join(&relative);
    let file_path = std::fs::canonicalize(&file_path).map_err(|_| ApiError::ApiNotFound {
        message: "Not Found".into(),
    })?;
    if !file_path.starts_with(&base) {
        return Err(ApiError::ApiNotFound {
            message: "Not Found".into(),
        });
    }
    let content = std::fs::read(&file_path).map_err(|_| ApiError::ApiNotFound {
        message: "Not Found".into(),
    })?;
    let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
    let mut response = Response::new(axum::body::Body::from(content));
    response.headers_mut().insert(
        "content-type",
        mime.to_string()
            .parse()
            .map_err(|_| ApiError::V1BadRequest)?,
    );
    Ok(response)
}

/// `fs.list` — list direct children of one directory. From
/// reference/packages/server/src/handlers/fs.ts.
pub async fn fs_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let path = params.get("path").cloned().unwrap_or_default();
    let dir = safe_directory(&location.directory, &path).ok_or_else(|| ApiError::ApiNotFound {
        message: "Not Found".into(),
    })?;
    let root = std::fs::canonicalize(&location.directory).map_err(|_| ApiError::ApiNotFound {
        message: "Not Found".into(),
    })?;
    let read = std::fs::read_dir(&dir).map_err(|_| ApiError::ApiNotFound {
        message: "Not Found".into(),
    })?;
    let mut entries = Vec::new();
    for entry in read.flatten() {
        let file_type = entry.file_type().ok();
        let Some(file_type) = file_type else {
            continue;
        };
        let entry_type = if file_type.is_dir() {
            "directory"
        } else if file_type.is_file() {
            "file"
        } else {
            continue;
        };
        let entry_path = entry.path();
        let Ok(relative) = entry_path.strip_prefix(&root) else {
            continue;
        };
        let mut relative = relative.to_string_lossy().replace('\\', "/");
        if entry_type == "directory" {
            relative.push('/');
        }
        entries.push((relative, entry_type));
    }
    entries.sort_by(|(left_path, left_type), (right_path, right_type)| {
        left_type
            .cmp(right_type)
            .then_with(|| left_path.cmp(right_path))
    });
    let entries = entries
        .into_iter()
        .map(|(path, entry_type)| serde_json::json!({ "path": path, "type": entry_type }))
        .collect::<Vec<_>>();
    json(&LocationResponse {
        location: location.info(),
        data: entries,
    })
}

/// `fs.find` — recursively ranked filesystem entries.
pub async fn fs_find(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let query = params.get("query").cloned().unwrap_or_default();
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);
    let pattern = if query.is_empty() {
        "*".to_string()
    } else {
        format!("*{query}*")
    };
    let kind = params.get("type").map(String::as_str).unwrap_or("file");
    let matches = match kind {
        "file" => oc_tool::ripgrep::find(&oc_tool::ripgrep::FindInput {
            cwd: location.directory.clone(),
            pattern,
            limit,
            hidden: false,
            follow: false,
        })
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| serde_json::json!({ "path": entry.path, "type": entry.kind }))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default(),
        "directory" => find_directories(&location.directory, &query, limit),
        _ => Vec::new(),
    };
    json(&LocationResponse {
        location: location.info(),
        data: matches,
    })
}

fn find_directories(directory: &str, query: &str, limit: usize) -> Vec<serde_json::Value> {
    let root = match std::fs::canonicalize(directory) {
        Ok(root) => root,
        Err(_) => return Vec::new(),
    };
    let mut pending = vec![root.clone()];
    let mut matches = Vec::new();
    while let Some(parent) = pending.pop() {
        let read = match std::fs::read_dir(&parent) {
            Ok(read) => read,
            Err(_) => continue,
        };
        let mut children = read.flatten().collect::<Vec<_>>();
        children.sort_by_key(|entry| entry.file_name());
        for entry in children.into_iter().rev() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if !matches!(entry.file_type(), Ok(file_type) if file_type.is_dir()) {
                continue;
            }
            let relative = match entry.path().strip_prefix(&root) {
                Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if query.is_empty() || relative.contains(query) {
                matches.push(serde_json::json!({
                    "path": format!("{relative}/"),
                    "type": "directory",
                }));
                if matches.len() >= limit {
                    return matches;
                }
            }
            pending.push(entry.path());
        }
    }
    matches
}

fn safe_directory(directory: &str, path: &str) -> Option<PathBuf> {
    let base = std::fs::canonicalize(directory).ok()?;
    let candidate = base.join(path);
    let canonical = std::fs::canonicalize(candidate).ok()?;
    canonical.starts_with(base).then_some(canonical)
}

/// Normalize the wildcard route's path without silently rewriting traversal
/// segments. The reference `RelativePath` schema rejects paths that escape
/// the selected location; returning `None` lets the handler expose the same
/// not-found boundary without leaking filesystem details.
fn safe_read_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let mut segments = Vec::new();
    for segment in normalized.trim_start_matches('/').split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return None;
        }
        segments.push(segment);
    }
    Some(segments.join("/"))
}
