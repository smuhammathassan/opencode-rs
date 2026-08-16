//! Project instance context.
//! Mirrors `src/project/instance-context.ts` and `src/cli/bootstrap.ts`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use oc_database::{database::path as database_path, tables::ProjectRow, Database};

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

fn git_project_id(worktree: &Path) -> Option<String> {
    let id = oc_project::identity::project_id(worktree);
    (id != "global").then_some(id)
}

fn project_id(worktree: &Path, vcs: &Vcs) -> String {
    match vcs {
        Vcs::Git => git_project_id(worktree).unwrap_or_else(|| "global".to_string()),
        Vcs::None => "global".to_string(),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn persist_project(database: &Database, project: &ProjectInfo) -> anyhow::Result<()> {
    let existing = database.get_project(&project.id)?;
    let now = now_ms();
    let mut row = existing.unwrap_or_else(|| ProjectRow {
        id: project.id.clone(),
        worktree: project.worktree.to_string_lossy().into_owned(),
        vcs: None,
        name: None,
        icon_url: None,
        icon_url_override: None,
        icon_color: None,
        time_created: now,
        time_updated: now,
        time_initialized: None,
        sandboxes: serde_json::json!([]),
        commands: None,
    });

    // Detection fields are refreshed on every load, while user-managed
    // metadata (name, icon, commands, sandboxes, initialized time) survives.
    row.worktree = project.worktree.to_string_lossy().into_owned();
    row.vcs = match &project.vcs {
        Vcs::Git => Some("git".to_string()),
        Vcs::None => None,
    };
    row.time_updated = now;
    database.upsert_project(&row)?;
    Ok(())
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
        let vcs = detect_vcs(&directory);
        let project = ProjectInfo {
            id: project_id(&worktree, &vcs),
            directory: directory.clone(),
            worktree: worktree.clone(),
            vcs,
        };
        let database = Database::open(database_path())?;
        persist_project(&database, &project)?;
        Ok(Context {
            directory,
            worktree,
            project,
            paths,
        })
    }

    /// Persist this context's detected project using an already-open database.
    /// Kept separate from the database handle so callers can control the
    /// connection lifetime while Context::load still mirrors CLI bootstrap.
    pub fn persist_project(&self, database: &Database) -> anyhow::Result<()> {
        persist_project(database, &self.project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_persistence_refreshes_detection_without_losing_metadata() {
        let database = Database::open_memory().expect("database");
        let project = ProjectInfo {
            id: "project-id".to_string(),
            directory: PathBuf::from("/repo/src"),
            worktree: PathBuf::from("/repo"),
            vcs: Vcs::Git,
        };

        persist_project(&database, &project).expect("initial persistence");
        let mut stored = database
            .get_project("project-id")
            .expect("lookup")
            .expect("row");
        stored.name = Some("My project".to_string());
        stored.icon_color = Some("#fff".to_string());
        stored.sandboxes = serde_json::json!(["/repo-sandbox"]);
        stored.time_initialized = Some(123);
        database.upsert_project(&stored).expect("metadata update");

        let refreshed = ProjectInfo {
            worktree: PathBuf::from("/repo-renamed"),
            vcs: Vcs::None,
            ..project
        };
        persist_project(&database, &refreshed).expect("refresh");
        let stored = database
            .get_project("project-id")
            .expect("lookup")
            .expect("row");

        assert_eq!(stored.worktree, "/repo-renamed");
        assert_eq!(stored.vcs, None);
        assert_eq!(stored.name.as_deref(), Some("My project"));
        assert_eq!(stored.icon_color.as_deref(), Some("#fff"));
        assert_eq!(stored.sandboxes, serde_json::json!(["/repo-sandbox"]));
        assert_eq!(stored.time_initialized, Some(123));
    }

    #[test]
    fn normalizes_reference_project_remote_identity() {
        assert_eq!(
            oc_project::identity::normalized_remote("git@GitHub.com:Owner/Repo.git"),
            Some("github.com/Owner/Repo".to_string())
        );
        assert_eq!(
            oc_project::identity::normalized_remote("https://GitHub.com/Owner/Repo.git/"),
            Some("github.com/Owner/Repo".to_string())
        );
        assert_eq!(
            oc_project::identity::normalized_remote("file:///tmp/repo"),
            None
        );
    }

    #[test]
    fn uses_repo_cache_before_root_commit_fallback() {
        let directory =
            std::env::temp_dir().join(format!("opencode-cli-project-cache-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&directory)
            .status()
            .unwrap();
        assert!(init.success());
        std::fs::write(directory.join(".git/opencode"), "cached-project\n").unwrap();

        assert_eq!(
            git_project_id(&directory),
            Some("cached-project".to_string())
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
