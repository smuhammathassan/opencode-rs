/// Local stand-in for the drizzle-backed `ProjectTable`/`SessionTable`/
/// `WorkspaceTable` access the reference `Project` service uses
/// (reference/packages/opencode/src/project/project.ts).
///
/// This in-memory store mirrors the exact operations the reference performs
/// against SQLite so the port's logic can be exercised in tests.
///
/// TODO(integration): replace with oc-database once it lands; the trait keeps
/// the call sites identical.
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::schema::{ProjectCommands, ProjectIcon, ProjectInfo, ProjectTime};

#[derive(Debug, Clone, Default)]
pub struct ProjectRow {
    pub id: String,
    pub worktree: String,
    pub vcs: Option<String>,
    pub name: Option<String>,
    pub icon_url: Option<String>,
    pub icon_url_override: Option<String>,
    pub icon_color: Option<String>,
    pub time_created: u64,
    pub time_updated: u64,
    pub time_initialized: Option<u64>,
    pub sandboxes: Vec<String>,
    pub commands: Option<ProjectCommands>,
}

/// Mirrors `Project.fromRow` (reference/packages/opencode/src/project/project.ts).
pub fn from_row(row: &ProjectRow) -> ProjectInfo {
    let icon = if row.icon_url.is_some() || row.icon_url_override.is_some() || row.icon_color.is_some() {
        Some(ProjectIcon {
            url: row.icon_url.clone(),
            override_: row.icon_url_override.clone(),
            color: row.icon_color.clone(),
        })
    } else {
        None
    };
    ProjectInfo {
        id: crate::schema::ProjectID::make(&row.id),
        worktree: row.worktree.clone(),
        vcs: row.vcs.clone(),
        name: row.name.clone(),
        icon,
        commands: row.commands.clone(),
        time: ProjectTime {
            created: row.time_created,
            updated: row.time_updated,
            initialized: row.time_initialized,
        },
        sandboxes: row.sandboxes.clone(),
    }
}

#[derive(Debug, Clone, Default)]
struct SessionRow {
    project_id: String,
    directory: String,
    #[allow(dead_code)]
    time_updated: u64,
}

#[derive(Debug, Clone, Default)]
struct WorkspaceRow {
    project_id: String,
}

#[derive(Debug, Default)]
struct State {
    projects: HashMap<String, ProjectRow>,
    project_directories: HashSet<(String, String)>,
    sessions: HashMap<String, SessionRow>,
    workspaces: HashMap<String, WorkspaceRow>,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectStore {
    state: std::sync::Arc<Mutex<State>>,
}

impl ProjectStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_project(&self, id: &str) -> Option<ProjectRow> {
        self.state.lock().unwrap().projects.get(id).cloned()
    }

    pub fn list_projects(&self) -> Vec<ProjectRow> {
        self.state.lock().unwrap().projects.values().cloned().collect()
    }

    /// Mirrors the `insert ... onConflictDoUpdate` upsert in `Project.fromDirectory`.
    pub fn upsert_project(&self, row: &ProjectRow) {
        let mut state = self.state.lock().unwrap();
        state.projects.insert(row.id.clone(), row.clone());
    }

    /// Mirrors `db.update(ProjectTable).set({ name, icon_url, ... }) .returning()`.
    pub fn update_project(
        &self,
        id: &str,
        name: Option<String>,
        icon: Option<&ProjectIcon>,
        commands: Option<ProjectCommands>,
        time_updated: u64,
    ) -> Option<ProjectRow> {
        let mut state = self.state.lock().unwrap();
        let row = state.projects.get_mut(id)?;
        row.name = name;
        row.icon_url = icon.and_then(|icon| icon.url.clone());
        row.icon_url_override = icon.and_then(|icon| icon.override_.clone());
        row.icon_color = icon.and_then(|icon| icon.color.clone());
        row.commands = commands;
        row.time_updated = time_updated;
        Some(row.clone())
    }

    /// Mirrors the sandbox-list update in `addSandbox`/`removeSandbox`.
    pub fn update_project_sandboxes(&self, id: &str, sandboxes: Vec<String>, time_updated: u64) -> Option<ProjectRow> {
        let mut state = self.state.lock().unwrap();
        let row = state.projects.get_mut(id)?;
        row.sandboxes = sandboxes;
        row.time_updated = time_updated;
        Some(row.clone())
    }

    pub fn set_project_initialized(&self, id: &str, time_initialized: u64) {
        let mut state = self.state.lock().unwrap();
        if let Some(row) = state.projects.get_mut(id) {
            row.time_initialized = Some(time_initialized);
        }
    }

    /// Mirrors the session re-assignment in `Project.fromDirectory`.
    pub fn update_sessions_to_project(&self, directory: &str, project_id: &str) {
        let mut state = self.state.lock().unwrap();
        for session in state.sessions.values_mut() {
            if session.project_id == "global" && session.directory == directory {
                session.project_id = project_id.to_string();
            }
        }
    }

    /// Mirrors `Project.migrateProjectId`: moves the old project's identity and
    /// children onto the new id (transactional in the reference).
    pub fn migrate_project_id(&self, old: &str, new: &str) {
        if old.is_empty() || old == "global" || old == new {
            return;
        }
        let mut state = self.state.lock().unwrap();
        let old_row = state.projects.get(old).cloned();
        let has_new = state.projects.contains_key(new);
        if let Some(old_project) = &old_row {
            if !has_new {
                let mut migrated = old_project.clone();
                migrated.id = new.to_string();
                migrated.time_updated = now();
                state.projects.insert(new.to_string(), migrated);
            }
        }

        state.project_directories.retain(|(project_id, _)| project_id != old);

        for session in state.sessions.values_mut() {
            if session.project_id == old {
                session.project_id = new.to_string();
            }
        }
        for workspace in state.workspaces.values_mut() {
            if workspace.project_id == old {
                workspace.project_id = new.to_string();
            }
        }

        if old_row.is_some() {
            state.projects.remove(old);
        }
    }

    /// Mirrors `ProjectDirectories.create` with `onConflictDoNothing`.
    pub fn add_project_directory(&self, project_id: &str, directory: &str) {
        let mut state = self.state.lock().unwrap();
        state.project_directories.insert((project_id.to_string(), directory.to_string()));
    }

    /// Test helper mirroring the session table's `project_id` + `directory`.
    pub fn insert_session(&self, session_id: &str, project_id: &str, directory: &str) {
        let mut state = self.state.lock().unwrap();
        state.sessions.insert(
            session_id.to_string(),
            SessionRow { project_id: project_id.to_string(), directory: directory.to_string(), time_updated: now() },
        );
    }

    /// Test helper mirroring the workspace table's `project_id`.
    pub fn insert_workspace(&self, workspace_id: &str, project_id: &str) {
        let mut state = self.state.lock().unwrap();
        state.workspaces.insert(workspace_id.to_string(), WorkspaceRow { project_id: project_id.to_string() });
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
