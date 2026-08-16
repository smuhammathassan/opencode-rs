//! Shared project identity resolution.
//!
//! This is the synchronous boundary used by the CLI and HTTP server. It
//! mirrors `ProjectV2.resolve`'s stable identity order so a project does not
//! change IDs depending on which Rust entry point discovered it.

use std::path::{Path, PathBuf};

/// Resolve a directory to the reference project ID order: origin remote,
/// repo-local cache, root commit, then the global project.
pub fn project_id(directory: &Path) -> String {
    let Some(worktree) = git_root(directory) else {
        return "global".to_string();
    };

    if let Some(remote) = git_remote(&worktree).and_then(|remote| normalized_remote(&remote)) {
        return crate::util::hash::Hash::fast(format!("git-remote:{remote}").as_bytes());
    }
    if let Some(cached) = git_cached_project_id(&worktree) {
        return cached;
    }
    git_root_commit(&worktree).unwrap_or_else(|| "global".to_string())
}

/// Normalize an origin URL to the stable `host/path` form used by OpenCode.
pub fn normalized_remote(input: &str) -> Option<String> {
    let value = input.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = url::Url::parse(value) {
        if parsed.scheme() == "file" {
            return None;
        }
        return normalized_remote_parts(parsed.host_str().unwrap_or_default(), parsed.path());
    }
    let colon = value.find(':')?;
    let at = value.find('@').map(|index| index + 1).unwrap_or(0);
    let host = value[at..colon]
        .rsplit('/')
        .next()
        .unwrap_or(&value[at..colon]);
    normalized_remote_parts(host, &value[colon + 1..])
}

fn normalized_remote_parts(host: &str, name: &str) -> Option<String> {
    let name = name.trim_start_matches('/').trim_end_matches('/');
    let name = name.strip_suffix(".git").unwrap_or(name);
    if host.is_empty() || name.is_empty() {
        None
    } else {
        Some(format!("{}/{}", host.to_lowercase(), name))
    }
}

fn git_root(directory: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(directory)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

fn git_remote(worktree: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(worktree)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!remote.is_empty()).then_some(remote)
}

fn git_cached_project_id(worktree: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(worktree)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let common = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if common.is_empty() {
        return None;
    }
    let common = Path::new(&common);
    let common = if common.is_absolute() {
        common.to_path_buf()
    } else {
        worktree.join(common)
    };
    let value = std::fs::read_to_string(common.join("opencode")).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn git_root_commit(worktree: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-list", "--max-parents=0", "HEAD"])
        .current_dir(worktree)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let mut roots = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    roots.sort();
    roots.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_reference_remotes() {
        assert_eq!(
            normalized_remote("git@GitHub.com:Owner/Repo.git"),
            Some("github.com/Owner/Repo".to_string())
        );
        assert_eq!(normalized_remote("file:///tmp/repo"), None);
    }
}
