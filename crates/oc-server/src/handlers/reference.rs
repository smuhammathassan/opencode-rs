//! Reference handler. From reference/packages/server/src/handlers/reference.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use crate::schema::LocationResponse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `reference.list()` from `reference/packages/server/src/handlers/reference.ts`.
///
/// References are declared by the resolved `references`/`reference` config
/// object. Local entries are resolved against the active directory; remote
/// entries are projected to their stable local cache path so clients can
/// display and later hydrate them without receiving an invented empty list.
pub async fn reference_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let config = state.stores.read().await.config.clone();
    let entries = config
        .get("references")
        .or_else(|| config.get("reference"))
        .and_then(serde_json::Value::as_object)
        .map(|references| {
            references
                .iter()
                .filter_map(|(name, entry)| reference_info(name, entry, &location.directory))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json(&LocationResponse {
        location: location.info(),
        data: entries,
    })
}

fn reference_info(
    name: &str,
    value: &serde_json::Value,
    directory: &str,
) -> Option<serde_json::Value> {
    let (source, path) = if let Some(path) = value.as_str() {
        if path.starts_with('.') || path.starts_with('/') || path.starts_with('~') {
            let path = resolve_path(directory, path);
            (serde_json::json!({ "type": "local", "path": path }), path)
        } else {
            let cache = cache_path(directory, name);
            (
                serde_json::json!({ "type": "git", "repository": path }),
                cache,
            )
        }
    } else if let Some(object) = value.as_object() {
        if let Some(local) = object.get("path").and_then(serde_json::Value::as_str) {
            let path = resolve_path(directory, local);
            (
                serde_json::json!({
                    "type": "local",
                    "path": path,
                    "description": object.get("description"),
                    "hidden": object.get("hidden"),
                }),
                path,
            )
        } else if let Some(repository) =
            object.get("repository").and_then(serde_json::Value::as_str)
        {
            let path = cache_path(directory, name);
            (
                serde_json::json!({
                    "type": "git",
                    "repository": repository,
                    "branch": object.get("branch"),
                    "description": object.get("description"),
                    "hidden": object.get("hidden"),
                }),
                path,
            )
        } else {
            return None;
        }
    } else {
        return None;
    };
    Some(serde_json::json!({
        "name": name,
        "path": path,
        "source": source,
    }))
}

fn resolve_path(directory: &str, value: &str) -> String {
    let expanded = value.strip_prefix("~/").map(|rest| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest)
    });
    let path = expanded.unwrap_or_else(|| {
        let path = Path::new(value);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new(directory).join(path)
        }
    });
    std::fs::canonicalize(&path)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn cache_path(directory: &str, name: &str) -> String {
    let safe_name: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    Path::new(directory)
        .join(".opencode")
        .join("references")
        .join(safe_name)
        .to_string_lossy()
        .into_owned()
}
