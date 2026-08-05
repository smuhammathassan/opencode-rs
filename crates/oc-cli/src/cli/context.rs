//! Project instance context.
//! Mirrors `src/project/instance-context.ts` and `src/cli/bootstrap.ts`.

use std::path::{Path, PathBuf};

use super::paths::GlobalPaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vcs {
    Git,
    None,
}

/// Minimal project info mirroring `Project.Info`.
/// TODO(integration): reuse `oc_core`'s project detection.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub id: String,
    pub directory: PathBuf,
    pub worktree: PathBuf,
    pub vcs: Vcs,
}

fn detect_vcs(dir: &Path) -> Vcs {
    if dir.join(".git").exists() || git_root(dir).is_some() {
        Vcs::Git
    } else {
        Vcs::None
    }
}

fn git_root(dir: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn stable_id(input: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// A loaded instance context: the directory + worktree a command runs in, the
/// global paths, and any per-project state.
#[derive(Debug, Clone)]
pub struct Context {
    pub directory: PathBuf,
    pub worktree: PathBuf,
    pub project: ProjectInfo,
    pub paths: GlobalPaths,
}

impl Context {
    /// Resolve the git worktree for a directory.
    pub fn resolve_worktree(directory: &Path) -> PathBuf {
        git_root(directory).unwrap_or_else(|| directory.to_path_buf())
    }

    /// Load an instance context for a directory, mirroring `bootstrap()`.
    pub fn load(directory: impl Into<PathBuf>) -> anyhow::Result<Context> {
        let directory = directory.into();
        let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
        let worktree = Self::resolve_worktree(&directory);
        let paths = GlobalPaths::load();
        let _ = paths.ensure();
        let project = ProjectInfo {
            id: stable_id(&worktree.to_string_lossy()),
            directory: directory.clone(),
            worktree: worktree.clone(),
            vcs: detect_vcs(&directory),
        };
        Ok(Context {
            directory,
            worktree,
            project,
            paths,
        })
    }
}
