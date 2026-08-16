//! Project-copy handler. From reference/packages/server/src/handlers/project-copy.ts.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{json, no_content, request_location, HandlerResult};
use crate::errors::ApiError;
use crate::schema::LocationResponse;
use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};

/// `ProjectCopy.create(...)` from `reference/packages/server/src/handlers/project-copy.ts`.
///
/// `clone` uses git's local clone path (preserving the repository metadata),
/// while `directory`/`copy` performs a symlink-safe recursive copy. Both
/// paths validate the requested destination before creating it and return the
/// location-wrapped `ProjectCopy.Copy` contract.
pub async fn project_copy_create(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let project_id = params
        .get("projectID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let strategy = body
        .get("strategy")
        .and_then(|value| value.as_str())
        .unwrap_or("clone")
        .to_ascii_lowercase();
    let source = canonical_source(&location.directory)?;
    let destination = requested_path(
        &location.directory,
        body.get("directory")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ApiError::InvalidRequest {
                message: "project copy directory is required".into(),
                kind: Some("projectCopy".into()),
                field: Some("directory".into()),
            })?,
    );
    validate_destination(&source, &destination)?;
    if destination.exists() {
        return Err(ApiError::Conflict {
            message: format!(
                "project copy destination already exists: {}",
                destination.display()
            ),
            resource: Some("projectCopy".into()),
        });
    }

    let result = match strategy.as_str() {
        "clone" => clone_repository(&source, &destination).await,
        "copy" | "directory" => copy_directory(&source, &destination),
        other => Err(ApiError::InvalidRequest {
            message: format!("unsupported project copy strategy: {other}"),
            kind: Some("projectCopy".into()),
            field: Some("strategy".into()),
        }),
    };
    if let Err(error) = result {
        let _ = remove_path(&destination);
        return Err(error);
    }
    let _ = project_id;
    let _ = body.get("name");
    json(&LocationResponse {
        location: location.info(),
        data: serde_json::json!({ "directory": destination }),
    })
}

pub async fn project_copy_remove(
    State(state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let source = canonical_source(&location.directory)?;
    let destination = requested_path(
        &location.directory,
        body.get("directory")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ApiError::InvalidRequest {
                message: "project copy directory is required".into(),
                kind: Some("projectCopy".into()),
                field: Some("directory".into()),
            })?,
    );
    validate_destination(&source, &destination)?;
    if !destination.exists() {
        return no_content();
    }
    let force = body
        .get("force")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !force
        && std::fs::read_dir(&destination)
            .ok()
            .and_then(|mut entries| entries.next())
            .is_some()
    {
        return Err(ApiError::Conflict {
            message: format!("project copy is not empty: {}", destination.display()),
            resource: Some("projectCopy".into()),
        });
    }
    remove_path(&destination).map_err(|error| ApiError::Unknown {
        message: format!("failed to remove project copy: {error}"),
        reference: None,
    })?;
    no_content()
}

pub async fn project_copy_refresh(
    State(_state): State<crate::state::AppState>,
    Path(_params): Path<HashMap<String, String>>,
) -> HandlerResult {
    no_content()
}

fn requested_path(directory: &str, value: &str) -> PathBuf {
    let path = FsPath::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        FsPath::new(directory).join(path)
    }
}

fn canonical_source(directory: &str) -> Result<PathBuf, ApiError> {
    std::fs::canonicalize(directory).map_err(|_| ApiError::ApiNotFound {
        message: format!("project source not found: {directory}"),
    })
}

fn validate_destination(source: &FsPath, destination: &FsPath) -> Result<(), ApiError> {
    if destination == source || source.starts_with(destination) || destination.starts_with(source) {
        return Err(ApiError::InvalidRequest {
            message: "project copy destination must be outside the source project".into(),
            kind: Some("projectCopy".into()),
            field: Some("directory".into()),
        });
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ApiError::Unknown {
            message: format!("failed to prepare project copy destination: {error}"),
            reference: None,
        })?;
        let parent = std::fs::canonicalize(parent).map_err(|error| ApiError::Unknown {
            message: format!("failed to resolve project copy destination: {error}"),
            reference: None,
        })?;
        if parent == source || parent.starts_with(source) {
            return Err(ApiError::InvalidRequest {
                message: "project copy destination must be outside the source project".into(),
                kind: Some("projectCopy".into()),
                field: Some("directory".into()),
            });
        }
    }
    Ok(())
}

async fn clone_repository(source: &FsPath, destination: &FsPath) -> Result<(), ApiError> {
    let output = tokio::process::Command::new("git")
        .args(["clone", "--local", "--no-hardlinks"])
        .arg(source)
        .arg(destination)
        .output()
        .await
        .map_err(|error| ApiError::Unknown {
            message: format!("failed to start git clone: {error}"),
            reference: None,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ApiError::Unknown {
            message: format!(
                "git clone failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            reference: None,
        })
    }
}

fn copy_directory(source: &FsPath, destination: &FsPath) -> Result<(), ApiError> {
    std::fs::create_dir_all(destination).map_err(|error| ApiError::Unknown {
        message: format!("failed to create project copy: {error}"),
        reference: None,
    })?;
    for entry in std::fs::read_dir(source).map_err(|error| ApiError::Unknown {
        message: format!("failed to read project source: {error}"),
        reference: None,
    })? {
        let entry = entry.map_err(|error| ApiError::Unknown {
            message: format!("failed to read project source entry: {error}"),
            reference: None,
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata =
            std::fs::symlink_metadata(&source_path).map_err(|error| ApiError::Unknown {
                message: format!("failed to inspect project source entry: {error}"),
                reference: None,
            })?;
        if metadata.file_type().is_symlink() {
            return Err(ApiError::InvalidRequest {
                message: format!(
                    "symlink entries are not allowed in project copies: {}",
                    source_path.display()
                ),
                kind: Some("projectCopy".into()),
                field: Some("strategy".into()),
            });
        }
        if metadata.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path).map_err(|error| ApiError::Unknown {
                message: format!("failed to copy {}: {error}", source_path.display()),
                reference: None,
            })?;
        }
    }
    Ok(())
}

fn remove_path(path: &FsPath) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
