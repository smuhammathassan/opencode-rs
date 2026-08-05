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
    let relative = path
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != "..")
        .collect::<Vec<_>>()
        .join("/");
    let file_path = PathBuf::from(&location.directory).join(&relative);
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
/// reference/packages/server/src/handlers/fs.ts. TODO(integration): delegate to
/// oc-core FileSystem service for full Entry serialization.
pub async fn fs_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let path = params.get("path").cloned().unwrap_or_default();
    let dir = PathBuf::from(&location.directory).join(path);
    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(&dir) {
        for entry in read.flatten() {
            let file_type = entry.file_type().ok();
            let is_dir = file_type.map(|t| t.is_dir()).unwrap_or(false);
            let name = entry.file_name().to_string_lossy().into_owned();
            entries.push(serde_json::json!({
                "name": name,
                "path": entry.path().to_string_lossy(),
                "type": if is_dir { "directory" } else { "file" },
                "mime": mime_guess::from_path(&entry.path()).first_or_octet_stream().to_string(),
            }));
        }
    }
    json(&LocationResponse {
        location: location.info(),
        data: entries,
    })
}

/// `fs.find` — recursively ranked filesystem entries. TODO(integration): ripgrep-backed
/// search via oc-util.
pub async fn fs_find(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let query = params.get("query").cloned().unwrap_or_default();
    let dir = PathBuf::from(&location.directory);
    let mut matches = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_lowercase().contains(&query.to_lowercase()) {
                matches.push(serde_json::json!({
                    "name": name,
                    "path": entry.path().to_string_lossy(),
                    "type": if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { "directory" } else { "file" },
                    "mime": mime_guess::from_path(&entry.path()).first_or_octet_stream().to_string(),
                }));
            }
        }
    }
    json(&LocationResponse {
        location: location.info(),
        data: matches,
    })
}
