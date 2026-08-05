/// From reference/packages/opencode/src/project/project.ts
use std::sync::Arc;

use crate::git::Git;
use crate::project::project_v2::ProjectV2;
use crate::project::store::{from_row, ProjectRow, ProjectStore};
use crate::schema::{ProjectID, ProjectIcon, ProjectInfo, ProjectNotFoundError, ProjectTime};
use crate::util::bus::{Bus, BusEvent, EventPayload};
use crate::util::config::Config;
use crate::util::process::SpawnOptions;
use crate::util::{fs, pathutil, process, GitResult};

pub const DEFAULT_INIT_COMMAND: &str = "init";

#[derive(Debug, Clone)]
pub struct FromDirectoryResult {
    pub project: ProjectInfo,
    pub sandbox: String,
}

#[derive(Clone)]
pub struct Project {
    pub git: Arc<Git>,
    pub v2: Arc<ProjectV2>,
    pub store: Arc<ProjectStore>,
    pub config: Arc<Config>,
    pub bus: Arc<Bus>,
}

impl Project {
    pub fn new(
        git: Arc<Git>,
        store: Arc<ProjectStore>,
        config: Arc<Config>,
        bus: Arc<Bus>,
    ) -> Arc<Project> {
        Arc::new(Project {
            v2: Arc::new(ProjectV2::new(git.clone())),
            git,
            store,
            config,
            bus,
        })
    }

    async fn git(&self, args: &[&str], cwd: Option<&str>) -> GitResult {
        let result = process::run(
            "git",
            args,
            SpawnOptions {
                cwd: cwd.map(String::from),
                ..Default::default()
            },
        )
        .await;
        match result {
            Ok(result) => GitResult {
                code: result.exit_code,
                text: result.stdout_text(),
                stderr: result.stderr_text(),
            },
            Err(error) => GitResult::failure(error.to_string()),
        }
    }

    fn emit_updated(&self, data: &ProjectInfo) {
        let properties = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
        self.bus.emit(BusEvent {
            directory: "global".to_string(),
            project: Some(data.id.0.clone()),
            workspace: None,
            payload: EventPayload {
                r#type: crate::schema::PROJECT_UPDATED.to_string(),
                properties: Some(properties),
                data: None,
                location: None,
            },
        });
    }

    /// Detects and attaches the project at `directory`, upserting its row and
    /// returning the project plus the "sandbox" (the path to run in).
    pub async fn from_directory(&self, directory: &str) -> anyhow::Result<FromDirectoryResult> {
        tracing::info!("fromDirectory");

        let data = self.v2.resolve(directory).await;
        let worktree = if data.id == "global" && data.vcs.is_none() {
            "/".to_string()
        } else {
            data.directory.clone()
        };

        // Phase 2: upsert
        let project_id = ProjectID::make(&data.id);
        self.store
            .migrate_project_id(data.previous.as_deref().unwrap_or(""), &data.id);
        let existing = self.store.get_project(&data.id).map(|row| from_row(&row));

        let fake_vcs = std::env::var("OPENCODE_FAKE_VCS").ok();
        let vcs = data
            .vcs
            .as_ref()
            .map(|vcs| vcs.r#type.clone())
            .or_else(|| fake_vcs.clone());
        let now = now_ms();

        let existing = existing.unwrap_or_else(|| ProjectInfo {
            id: project_id.clone(),
            worktree: worktree.clone(),
            vcs,
            sandboxes: Vec::new(),
            time: ProjectTime {
                created: now,
                updated: now,
                initialized: None,
            },
            ..ProjectInfo::default()
        });

        if self.config.experimental_icon_discovery {
            let project = self.clone();
            let info = existing.clone();
            tokio::spawn(async move {
                let _ = project.discover(&info).await;
            });
        }

        let mut result = ProjectInfo {
            worktree: if project_id == ProjectID::global() {
                worktree.clone()
            } else {
                existing.worktree.clone()
            },
            vcs: data
                .vcs
                .as_ref()
                .map(|vcs| vcs.r#type.clone())
                .or_else(|| fake_vcs.clone()),
            time: ProjectTime {
                created: existing.time.created,
                updated: now,
                initialized: existing.time.initialized,
            },
            ..existing
        };

        if project_id != ProjectID::global()
            && data.directory != result.worktree
            && !result.sandboxes.contains(&data.directory)
        {
            result.sandboxes.push(data.directory.clone());
        }
        result.sandboxes = self.filter_existing(&result.sandboxes).await;

        let row = ProjectRow {
            id: result.id.0.clone(),
            worktree: pathutil::resolve(&result.worktree),
            vcs: result.vcs.clone(),
            name: result.name.clone(),
            icon_url: result.icon.as_ref().and_then(|icon| icon.url.clone()),
            icon_url_override: result.icon.as_ref().and_then(|icon| icon.override_.clone()),
            icon_color: result.icon.as_ref().and_then(|icon| icon.color.clone()),
            time_created: result.time.created,
            time_updated: result.time.updated,
            time_initialized: result.time.initialized,
            sandboxes: result
                .sandboxes
                .iter()
                .map(|sandbox| pathutil::resolve(sandbox))
                .collect(),
            commands: result.commands.clone(),
        };
        self.store.upsert_project(&row);

        if project_id != ProjectID::global() {
            self.store
                .update_sessions_to_project(&data.directory, &project_id.0);
        }

        self.store
            .add_project_directory(&project_id.0, &data.directory);

        self.emit_updated(&result);
        if project_id != ProjectID::global() {
            if let Some(vcs) = &data.vcs {
                if vcs.r#type == "git" {
                    self.v2.commit(&vcs.store, &data.id).await;
                }
            }
        }

        let sandbox = if data.vcs.is_some() {
            data.directory
        } else {
            worktree
        };
        Ok(FromDirectoryResult {
            project: result,
            sandbox,
        })
    }

    async fn filter_existing(&self, sandboxes: &[String]) -> Vec<String> {
        let mut kept = Vec::new();
        for sandbox in sandboxes {
            if fs::exists(sandbox).await {
                kept.push(sandbox.clone());
            }
        }
        kept
    }

    /// Discovers a favicon for a git project and stores it as a data URL.
    pub async fn discover(&self, input: &ProjectInfo) {
        if input.vcs.as_deref() != Some("git") {
            return;
        }
        if input
            .icon
            .as_ref()
            .is_some_and(|icon| icon.override_.is_some())
        {
            return;
        }
        if input.icon.as_ref().is_some_and(|icon| icon.url.is_some()) {
            return;
        }

        let mut matches = fs::glob_files(
            &input.worktree,
            &["ico", "png", "svg", "jpg", "jpeg", "webp"],
        );
        matches.sort_by(|a, b| a.len().cmp(&b.len()));
        let Some(shortest) = matches.into_iter().next() else {
            return;
        };

        let buffer = fs::read_bytes(&shortest).await;
        let base64 = base64_encode(&buffer);
        let mime = mime_type(&shortest);
        let url = format!("data:{mime};base64,{base64}");
        let update = ProjectUpdateInput {
            projectID: input.id.clone(),
            icon: Some(ProjectIcon {
                url: Some(url),
                override_: None,
                color: None,
            }),
            ..ProjectUpdateInput::default()
        };
        if let Err(error) = self.update(&update).await {
            tracing::debug!("favicon discovery update failed: {}", error);
        }
    }

    pub async fn list(&self) -> Vec<ProjectInfo> {
        self.store.list_projects().iter().map(from_row).collect()
    }

    pub async fn get(&self, id: &ProjectID) -> Option<ProjectInfo> {
        self.store.get_project(&id.0).map(|row| from_row(&row))
    }

    pub async fn update(
        &self,
        input: &ProjectUpdateInput,
    ) -> Result<ProjectInfo, ProjectNotFoundError> {
        let Some(result) = self.store.update_project(
            &input.projectID.0,
            input.name.clone(),
            input.icon.as_ref(),
            input.commands.clone(),
            now_ms(),
        ) else {
            return Err(ProjectNotFoundError::new(input.projectID.clone()));
        };
        let data = from_row(&result);
        self.emit_updated(&data);
        Ok(data)
    }

    pub async fn init_git(
        &self,
        directory: &str,
        project: &ProjectInfo,
    ) -> anyhow::Result<ProjectInfo> {
        if project.vcs.as_deref() == Some("git") {
            return Ok(project.clone());
        }
        if !process::which("git") {
            anyhow::bail!("Git is not installed");
        }
        let result = self.git(&["init", "--quiet"], Some(directory)).await;
        if result.code != 0 {
            let message = result.stderr.trim().to_string();
            let message = if message.is_empty() {
                result.text.trim().to_string()
            } else {
                message
            };
            anyhow::bail!(if message.is_empty() {
                "Failed to initialize git repository".to_string()
            } else {
                message
            });
        }
        Ok(self.from_directory(directory).await?.project)
    }

    pub async fn set_initialized(&self, id: &ProjectID) {
        self.store.set_project_initialized(&id.0, now_ms());
    }

    pub async fn sandboxes(&self, id: &ProjectID) -> Vec<String> {
        let Some(row) = self.store.get_project(&id.0) else {
            return Vec::new();
        };
        let data = from_row(&row);
        let mut kept = Vec::new();
        for sandbox in &data.sandboxes {
            if fs::is_dir(sandbox).await {
                kept.push(sandbox.clone());
            }
        }
        kept
    }

    pub async fn add_sandbox(&self, id: &ProjectID, directory: &str) -> anyhow::Result<()> {
        let row = self
            .store
            .get_project(&id.0)
            .ok_or_else(|| anyhow::anyhow!("Project not found: {id}"))?;
        let sandbox = pathutil::resolve(directory);
        let mut sandboxes = row.sandboxes.clone();
        if !sandboxes.contains(&sandbox) {
            sandboxes.push(sandbox);
        }
        let result = self
            .store
            .update_project_sandboxes(&id.0, sandboxes, now_ms())
            .ok_or_else(|| anyhow::anyhow!("Project not found: {id}"))?;
        self.emit_updated(&from_row(&result));
        Ok(())
    }

    pub async fn remove_sandbox(&self, id: &ProjectID, directory: &str) -> anyhow::Result<()> {
        let row = self
            .store
            .get_project(&id.0)
            .ok_or_else(|| anyhow::anyhow!("Project not found: {id}"))?;
        let sandbox = pathutil::resolve(directory);
        let sandboxes: Vec<String> = row
            .sandboxes
            .iter()
            .filter(|s| **s != sandbox)
            .cloned()
            .collect();
        let result = self
            .store
            .update_project_sandboxes(&id.0, sandboxes, now_ms())
            .ok_or_else(|| anyhow::anyhow!("Project not found: {id}"))?;
        self.emit_updated(&from_row(&result));
        Ok(())
    }

    /// Per-instance setup. Subscribes to the `/init` slash command for the
    /// current instance and stamps the project's initialized timestamp when it
    /// fires. The task exits when the instance's shutdown channel closes.
    pub fn init(
        &self,
        ctx: crate::project::instance_context::InstanceContext,
        shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        let project = self.clone();
        tokio::spawn(async move {
            let mut listener = project.bus.listener();
            let mut shutdown = shutdown;
            loop {
                let event = tokio::select! {
                    event = listener.next(|r#type, location| {
                        r#type == crate::schema::COMMAND_EXECUTED
                            && location == Some(ctx.directory.as_str())
                    }) => event,
                    _ = shutdown.recv() => break,
                };
                let Some(event) = event else { break };
                let Some(data) = event.payload.data.as_ref() else {
                    continue;
                };
                let name = data.get("name").and_then(|value| value.as_str());
                if name == Some(DEFAULT_INIT_COMMAND) {
                    project.set_initialized(&ctx.project.id).await;
                }
            }
        })
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn base64_encode(input: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(input)
}

fn mime_type(path: &str) -> String {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    match extension.to_lowercase().as_str() {
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "webp" => "image/webp".to_string(),
        "ico" => "image/x-icon".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

pub use crate::project::instance_context::InstanceContext;
pub use crate::schema::ProjectUpdateInput;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ProjectCommands, ProjectIcon};

    #[test]
    fn from_row_maps_all_columns() {
        let row = ProjectRow {
            id: "pid".to_string(),
            worktree: "/wt".to_string(),
            vcs: Some("git".to_string()),
            name: Some("n".to_string()),
            icon_url: Some("u".to_string()),
            icon_url_override: None,
            icon_color: Some("c".to_string()),
            time_created: 1,
            time_updated: 2,
            time_initialized: Some(3),
            sandboxes: vec!["/a".to_string()],
            commands: Some(ProjectCommands {
                start: Some("npm run dev".to_string()),
            }),
        };
        let info = from_row(&row);
        assert_eq!(info.id.0, "pid");
        assert_eq!(info.worktree, "/wt");
        assert_eq!(info.vcs.as_deref(), Some("git"));
        assert_eq!(info.name.as_deref(), Some("n"));
        assert_eq!(
            info.icon,
            Some(ProjectIcon {
                url: Some("u".to_string()),
                override_: None,
                color: Some("c".to_string())
            })
        );
        assert_eq!(info.time.created, 1);
        assert_eq!(info.time.updated, 2);
        assert_eq!(info.time.initialized, Some(3));
        assert_eq!(info.sandboxes, vec!["/a"]);
        assert_eq!(
            info.commands.as_ref().and_then(|c| c.start.as_deref()),
            Some("npm run dev")
        );
    }

    #[test]
    fn from_row_omits_empty_icon() {
        let row = ProjectRow {
            id: "pid".to_string(),
            worktree: "/wt".to_string(),
            ..ProjectRow::default()
        };
        let info = from_row(&row);
        assert_eq!(info.icon, None);
    }
}
