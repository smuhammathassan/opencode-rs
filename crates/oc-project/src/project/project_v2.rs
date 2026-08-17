/// From reference/packages/core/src/project.ts (`ProjectV2`)
///
/// Resolves a directory to a project identity. This lives in core in the
/// reference but oc-core is still a stub, so it is ported here for
/// `Project.fromDirectory`.
///
/// TODO(integration): move to oc-core once its ProjectV2 service lands.
use std::path::PathBuf;
use std::sync::Arc;

use crate::git::Git;
use crate::util::{fs, hash::Hash, pathutil};

#[derive(Debug, Clone)]
pub struct Vcs {
    pub r#type: String,
    pub store: String,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub previous: Option<String>,
    pub id: String,
    pub directory: String,
    pub vcs: Option<Vcs>,
}

pub struct ProjectV2 {
    pub git: Arc<Git>,
}

impl ProjectV2 {
    pub fn new(git: Arc<Git>) -> Self {
        ProjectV2 { git }
    }

    pub async fn resolve(&self, input: &str) -> Resolved {
        let repo = discover(self.git.clone(), input).await;
        let Some(repo) = repo else {
            return Resolved {
                previous: None,
                id: "global".to_string(),
                directory: root_of(input),
                vcs: None,
            };
        };

        let previous = cached(&repo.common_directory).await;
        let remote_id = remote(self.git.clone(), &repo).await;
        let root_id = root(self.git.clone(), &repo).await;
        let id = remote_id
            .or_else(|| previous.clone())
            .or(root_id)
            .unwrap_or_else(|| "global".to_string());

        Resolved {
            previous,
            id,
            directory: repo.worktree,
            vcs: Some(Vcs {
                r#type: "git".to_string(),
                store: repo.common_directory,
            }),
        }
    }

    /// Writes the resolved project id to the repo-local `opencode` cache file.
    pub async fn commit(&self, store: &str, id: &str) {
        let _ = fs::write_string(&pathutil::join(&[store, "opencode"]), id).await;
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Repository {
    worktree: String,
    git_directory: String,
    common_directory: String,
}

async fn cached(dir: &str) -> Option<String> {
    let value = fs::read_to_string(&pathutil::join(&[dir, "opencode"])).await;
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

async fn remote(git: Arc<Git>, repo: &Repository) -> Option<String> {
    let origin = git.remote_get_url(&repo.worktree, "origin").await?;
    let normalized = url(&origin)?;
    Some(Hash::fast(format!("git-remote:{normalized}").as_bytes()))
}

fn url(input: &str) -> Option<String> {
    let value = input.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = url::Url::parse(value) {
        if parsed.scheme() == "file" {
            return None;
        }
        return parts(parsed.host_str().unwrap_or(""), parsed.path());
    }
    let scp = value;
    let colon = scp.find(':')?;
    let at = scp.find('@').map(|i| i + 1).unwrap_or(0);
    let host_part = &scp[at..colon];
    let host = host_part.rsplit('/').next().unwrap_or(host_part);
    let path = &scp[colon + 1..];
    parts(host, path)
}

fn parts(host: &str, name: &str) -> Option<String> {
    let pathname = name
        .trim_start_matches('/')
        .strip_suffix(".git")
        .unwrap_or(name.trim_start_matches('/'))
        .trim_end_matches('/');
    if host.is_empty() || pathname.is_empty() {
        return None;
    }
    Some(format!("{}/{}", host.to_lowercase(), pathname))
}

async fn root(git: Arc<Git>, repo: &Repository) -> Option<String> {
    let result = git.root_commits(&repo.worktree).await;
    result.into_iter().next()
}

async fn discover(git: Arc<Git>, input: &str) -> Option<Repository> {
    let dotgit = up_target(".git", input).await?;
    let cwd = pathutil::dirname(&dotgit);
    let top_level = git
        .run(
            &["rev-parse", "--show-toplevel"],
            &crate::git::Options {
                cwd: cwd.clone(),
                ..Default::default()
            },
        )
        .await;
    let git_dir = git
        .run(
            &["rev-parse", "--git-dir"],
            &crate::git::Options {
                cwd: cwd.clone(),
                ..Default::default()
            },
        )
        .await;
    let common_dir = git
        .run(
            &["rev-parse", "--git-common-dir"],
            &crate::git::Options {
                cwd: cwd.clone(),
                ..Default::default()
            },
        )
        .await;
    if git_dir.exit_code != 0 || common_dir.exit_code != 0 {
        return None;
    }
    Some(Repository {
        worktree: if top_level.exit_code == 0 {
            resolve_path(&cwd, &top_level.text())
        } else {
            cwd.clone()
        },
        git_directory: resolve_path(&cwd, &git_dir.text()),
        common_directory: resolve_path(&cwd, &common_dir.text()),
    })
}

async fn up_target(target: &str, start: &str) -> Option<String> {
    let mut current = PathBuf::from(start);
    loop {
        let search = current.join(target);
        if fs::exists(search.to_str().unwrap_or_default()).await {
            return Some(search.to_string_lossy().into_owned());
        }
        let parent = current.parent();
        match parent {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return None,
        }
    }
}

fn resolve_path(cwd: &str, value: &str) -> String {
    let trimmed = value.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return cwd.to_string();
    }
    let normalized = windows_path(trimmed);
    let path = PathBuf::from(&normalized);
    if path.is_absolute() {
        pathutil::normalize(&normalized)
    } else {
        pathutil::resolve(&pathutil::join(&[cwd, &normalized]))
    }
}

fn windows_path(input: &str) -> String {
    if !cfg!(target_os = "windows") {
        return input.to_string();
    }
    let mut value = input.replace('/', "\\");
    if let Some(rest) = value.strip_prefix("\\\\") {
        value = format!("\\\\{}", rest);
    }
    value
}

fn root_of(input: &str) -> String {
    let path = PathBuf::from(input);
    match path.components().next() {
        Some(std::path::Component::RootDir) => "/".to_string(),
        Some(std::path::Component::Prefix(prefix)) => {
            prefix.as_os_str().to_string_lossy().into_owned()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parses_https_remotes() {
        assert_eq!(
            url("https://github.com/sst/opencode.git"),
            Some("github.com/sst/opencode".to_string())
        );
        assert_eq!(
            url("https://github.com/sst/opencode"),
            Some("github.com/sst/opencode".to_string())
        );
    }

    #[test]
    fn url_parses_scp_remotes_with_user() {
        // SCP forms without `user@` parse as a URL with an empty host in the
        // reference, so they resolve to `undefined`.
        assert_eq!(
            url("git@github.com:sst/opencode.git"),
            Some("github.com/sst/opencode".to_string())
        );
        assert_eq!(url("github.com:sst/opencode.git"), None);
    }

    #[test]
    fn url_rejects_file_and_empty() {
        assert_eq!(url("file:///tmp/repo"), None);
        assert_eq!(url("   "), None);
    }
}
