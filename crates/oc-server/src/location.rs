//! Location service, resolution, and the `{ location, data }` response wrapper.
//! From reference/packages/server/src/location.ts.

use std::path::PathBuf;

use crate::schema::{LocationInfo, LocationRef, ProjectRef};

/// Active location resolved from the request or the server default.
#[derive(Debug, Clone)]
pub struct Location {
    pub directory: String,
    pub workspace_id: Option<String>,
    pub project_id: String,
}

impl Location {
    pub fn default_location() -> Self {
        let directory = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .into_owned();
        Location {
            project_id: project_id(&directory),
            directory,
            workspace_id: None,
        }
    }

    pub fn with_directory(directory: &str, workspace_id: Option<&str>) -> Self {
        Location {
            project_id: project_id(directory),
            directory: directory.to_string(),
            workspace_id: workspace_id.map(|w| w.to_string()),
        }
    }

    pub fn info(&self) -> LocationInfo {
        LocationInfo {
            directory: self.directory.clone(),
            workspace_id: self.workspace_id.clone(),
            project: ProjectRef {
                id: self.project_id.clone(),
                directory: self.directory.clone(),
            },
        }
    }

    pub fn reference(&self) -> LocationRef {
        LocationRef {
            directory: self.directory.clone(),
            workspace_id: self.workspace_id.clone(),
        }
    }
}

fn project_id(directory: &str) -> String {
    oc_project::identity::project_id(std::path::Path::new(directory))
}

/// Resolve a location ref from request query/headers. From
/// reference/packages/server/src/location.ts (`ref`): `location[workspace]` query or
/// `x-opencode-workspace` header; `location[directory]` query or `x-opencode-directory`
/// header (decoded), else the current working directory.
pub fn resolve_location(
    query: Option<&str>,
    headers: &axum::http::HeaderMap,
    default: &Location,
) -> Location {
    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query.unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    let workspace_id = params
        .get("location[workspace]")
        .cloned()
        .or_else(|| header_value(headers, "x-opencode-workspace"));
    let directory = params
        .get("location[directory]")
        .cloned()
        .map(|d| decode(d))
        .or_else(|| header_value(headers, "x-opencode-directory").map(|d| decode(d)))
        .unwrap_or_else(|| default.directory.clone());
    Location::with_directory(&directory, workspace_id.as_deref())
}

fn header_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
}

fn decode(input: String) -> String {
    percent_encoding::percent_decode_str(&input)
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or(input)
}

/// Wrap data with the active location (`Location.response`).
pub fn located<T: serde::Serialize>(
    location: &Location,
    data: T,
) -> crate::schema::LocationResponse<T> {
    crate::schema::LocationResponse {
        location: location.info(),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_from_query() {
        let default = Location::default_location();
        let headers = axum::http::HeaderMap::new();
        let query = axum::extract::RawQuery(Some(
            "location%5Bdirectory%5D=%2Ftmp%2Fproj&location%5Bworkspace%5D=ws_1".into(),
        ));
        let location = resolve_location(query.0.as_deref(), &headers, &default);
        assert_eq!(location.directory, "/tmp/proj");
        assert_eq!(location.workspace_id.as_deref(), Some("ws_1"));
    }

    #[test]
    fn resolves_from_headers() {
        let default = Location::default_location();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-opencode-directory",
            axum::http::HeaderValue::from_static("/tmp%20proj"),
        );
        headers.insert(
            "x-opencode-workspace",
            axum::http::HeaderValue::from_static("ws_2"),
        );
        let location = resolve_location(None, &headers, &default);
        assert_eq!(location.directory, "/tmp proj");
        assert_eq!(location.workspace_id.as_deref(), Some("ws_2"));
    }

    #[test]
    fn git_project_id_uses_normalized_origin_remote() {
        let directory = std::env::temp_dir().join(format!(
            "opencode-location-project-id-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&directory)
            .status()
            .unwrap();
        assert!(init.success());
        let remote = std::process::Command::new("git")
            .args(["remote", "add", "origin", "git@GitHub.com:Owner/Repo.git"])
            .current_dir(&directory)
            .status()
            .unwrap();
        assert!(remote.success());
        let expected = oc_project::util::hash::Hash::fast(b"git-remote:github.com/Owner/Repo");
        assert_eq!(project_id(&directory.to_string_lossy()), expected);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn git_project_id_uses_repo_cache_when_origin_is_missing() {
        let directory = std::env::temp_dir().join(format!(
            "opencode-location-project-cache-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&directory)
            .status()
            .unwrap();
        assert!(init.success());
        std::fs::write(directory.join(".git/opencode"), "cached-project\n").unwrap();

        assert_eq!(project_id(&directory.to_string_lossy()), "cached-project");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn non_git_project_uses_global_identity() {
        let directory =
            std::env::temp_dir().join(format!("opencode-location-global-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(project_id(&directory.to_string_lossy()), "global");
        let _ = std::fs::remove_dir_all(directory);
    }
}
