//! Project detection service.
//!
//! From reference/packages/core/src/project.ts.
//!
//! Resolves a project ID for a directory by probing its git repository:
//! remote URL hash, then the repo-local `opencode` cache, then the root commit
//! hash.

pub mod directories;
pub mod schema;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fs_util::FSUtilService;
use crate::git::{GitError, GitService};
use crate::ids::ProjectId;
use crate::project::directories::ProjectDirectoriesService;
use crate::schema::AbsolutePath;
use crate::util::hash;

pub use crate::project::schema::ProjectVcs;

/// `Project.Info` — `{ id }`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectInfo {
    pub id: ProjectId,
}

/// `Project.Resolved` — `{ previous?, id, directory, vcs? }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub previous: Option<ProjectId>,
    pub id: ProjectId,
    pub directory: AbsolutePath,
    pub vcs: Option<ProjectVcs>,
}

/// The project service (`@opencode/ProjectV2`).
#[derive(Clone)]
pub struct ProjectService {
    fs: Arc<FSUtilService>,
    git: Arc<GitService>,
    directories: Arc<ProjectDirectoriesService>,
}

impl ProjectService {
    pub fn new(
        fs: Arc<FSUtilService>,
        git: Arc<GitService>,
        directories: Arc<ProjectDirectoriesService>,
    ) -> Self {
        ProjectService {
            fs,
            git,
            directories,
        }
    }

    /// `Project.directories(input)`.
    pub async fn directories(
        &self,
        input: &DirectoriesInput,
    ) -> Vec<crate::project::directories::Directory> {
        self.directories.list(&input.project_id).await
    }

    /// `Project.resolve(input)`.
    pub async fn resolve(&self, input: &AbsolutePath) -> Result<Resolved, GitError> {
        let repo = self.git.discover(&input.0).await;
        let Some(repo) = repo else {
            return Ok(Resolved {
                previous: None,
                id: ProjectId::global(),
                directory: AbsolutePath(path_root(&input.0)),
                vcs: None,
            });
        };

        let previous = self.cached(&repo.commonDirectory.0).await;
        let remote_id = self.remote(&repo).await?;
        let root_id = self.root(&repo).await;
        let id = remote_id.or(previous.clone()).or(root_id);
        Ok(Resolved {
            previous,
            id: id.unwrap_or_else(ProjectId::global),
            directory: repo.worktree,
            vcs: Some(ProjectVcs::git(repo.commonDirectory)),
        })
    }

    /// `Project.commit({ store, id })` — write the resolved ID to the
    /// repo-local cache.
    pub async fn commit(&self, store: &AbsolutePath, id: &ProjectId) {
        let path = Path::new(&store.0).join("opencode");
        let _ = self
            .fs
            .write_file_string(&path.display().to_string(), &id.0)
            .await;
    }

    async fn cached(&self, dir: &str) -> Option<ProjectId> {
        let path = Path::new(dir).join("opencode");
        let value = self
            .fs
            .read_file_string_safe(&path.display().to_string())
            .await?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(ProjectId::make(trimmed.to_string()))
        }
    }

    async fn remote(&self, repo: &crate::git::Repository) -> Result<Option<ProjectId>, GitError> {
        let origin = self.git.remote_get(repo, "origin").await;
        let Some(origin) = origin else {
            return Ok(None);
        };
        let Some(normalized) = remote_url(&origin) else {
            return Ok(None);
        };
        Ok(Some(ProjectId::make(hash::fast(
            format!("git-remote:{normalized}").as_bytes(),
        ))))
    }

    async fn root(&self, repo: &crate::git::Repository) -> Option<ProjectId> {
        let roots = self.git.history_root_commits(repo).await;
        roots.first().map(|root| ProjectId::make(root.clone()))
    }
}

/// `Project.directories` input.
#[derive(Debug, Clone)]
pub struct DirectoriesInput {
    pub project_id: ProjectId,
}

/// Normalize a git remote URL into `host/path` (lowercased host).
/// Mirrors the `url(...)` + `parts(...)` helpers in the reference.
pub fn remote_url(input: &str) -> Option<String> {
    let value = input.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = url::Url::parse(value) {
        if parsed.scheme() == "file" {
            return None;
        }
        return remote_parts(parsed.host_str().unwrap_or_default(), parsed.path());
    }
    // scp-like syntax: `user@host:path`
    let scp = value.split_once(':').map(|(host, path)| {
        let host = host.rsplit('@').next().unwrap_or("").to_string();
        (host, path)
    });
    match scp {
        Some((host, path)) if !host.contains('/') => remote_parts(&host, path),
        _ => None,
    }
}

fn remote_parts(host: &str, name: &str) -> Option<String> {
    let pathname = name.trim_start_matches('/');
    let pathname = pathname
        .strip_suffix(".git")
        .map(|p| p.to_string())
        .unwrap_or_else(|| pathname.to_string());
    let pathname = pathname.trim_end_matches('/').to_string();
    if host.is_empty() || pathname.is_empty() {
        return None;
    }
    Some(format!("{}/{pathname}", host.to_lowercase()))
}

fn path_root(input: &str) -> String {
    match Path::new(input).components().next() {
        Some(component) => PathBuf::from(component.as_os_str()).display().to_string(),
        None => "/".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_https_remote() {
        assert_eq!(
            remote_url("https://github.com/sst/opencode.git"),
            Some("github.com/sst/opencode".to_string())
        );
        assert_eq!(
            remote_url("  https://GitHub.com/sst/opencode/  "),
            Some("github.com/sst/opencode".to_string())
        );
    }

    #[test]
    fn normalizes_scp_remote() {
        assert_eq!(
            remote_url("git@github.com:sst/opencode.git"),
            Some("github.com/sst/opencode".to_string())
        );
        assert_eq!(
            remote_url("git@github.com:sst/opencode.git"),
            Some("github.com/sst/opencode".to_string())
        );
    }

    #[test]
    fn rejects_file_and_empty() {
        assert_eq!(remote_url("file:///repo"), None);
        assert_eq!(remote_url("  "), None);
        assert_eq!(remote_url("github.com/opencode"), None);
    }

    #[test]
    fn path_root_of_absolute() {
        assert_eq!(path_root("/a/b/c"), "/");
    }
}
