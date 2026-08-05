/// From reference/packages/opencode/src/worktree/index.ts
use std::sync::Arc;

use regex::Regex;

use crate::git::Git;
use crate::project::instance_context::InstanceContext;
use crate::project::instance_store::InstanceStore;
use crate::project::project::Project;
use crate::schema::{WorktreeCreateInput, WorktreeError, WorktreeInfo, WorktreeRemoveInput, WorktreeResetInput};
use crate::util::bus::{Bus, BusEvent, EventPayload};
use crate::util::global::Global;
use crate::util::pathutil;
use crate::util::process::{self, SpawnOptions};
use crate::util::slug::Slug;
use crate::util::{fs, GitResult};

const MAX_NAME_ATTEMPTS: usize = 26;

fn slugify(input: &str) -> String {
    let re = Regex::new(r"[^a-z0-9]+").unwrap();
    let value = input.trim().to_lowercase();
    let value = re.replace_all(&value, "-").to_string();
    value.trim_matches('-').to_string()
}

fn failed_removes(chunks: &[String]) -> Vec<String> {
    let re = Regex::new(r"(?i)^warning:\s+failed to remove\s+(.+):\s+").unwrap();
    let mut out = Vec::new();
    for chunk in chunks.iter().filter(|chunk| !chunk.is_empty()) {
        for line in chunk.split('\n') {
            let line = line.trim();
            let Some(captures) = re.captures(line) else { continue };
            let value = captures.get(1).map(|m| m.as_str()).unwrap_or_default().trim();
            let value = value.trim_matches(['\'', '"']);
            if value.is_empty() {
                continue;
            }
            out.push(value.to_string());
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
struct WorktreeEntry {
    path: Option<String>,
    branch: Option<String>,
}

fn parse_worktree_list(text: &str) -> Vec<WorktreeEntry> {
    let mut entries: Vec<WorktreeEntry> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            entries.push(WorktreeEntry { path: Some(path.trim().to_string()), branch: None });
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(current) = entries.last_mut() {
                current.branch = Some(branch.trim().to_string());
            }
        }
    }
    entries
}

pub struct Worktree {
    pub git: Arc<Git>,
    pub project: Arc<Project>,
    pub store: InstanceStore,
    pub bus: Arc<Bus>,
}

impl Clone for Worktree {
    fn clone(&self) -> Self {
        Worktree { git: self.git.clone(), project: self.project.clone(), store: self.store.clone(), bus: self.bus.clone() }
    }
}

impl Worktree {
    pub fn new(git: Arc<Git>, project: Arc<Project>, store: InstanceStore, bus: Arc<Bus>) -> Arc<Worktree> {
        Arc::new(Worktree { git, project, store, bus })
    }

    async fn git(&self, args: &[&str], cwd: Option<&str>) -> GitResult {
        let result = process::run("git", args, SpawnOptions { cwd: cwd.map(String::from), ..Default::default() }).await;
        match result {
            Ok(result) => GitResult { code: result.exit_code, text: result.stdout_text(), stderr: result.stderr_text() },
            Err(error) => GitResult::failure(error.to_string()),
        }
    }

    async fn canonical(&self, input: &str) -> String {
        let abs = pathutil::resolve(input);
        let real = fs::realpath(&abs).await.unwrap_or_else(|| abs.clone());
        let normalized = pathutil::normalize(&real);
        if cfg!(target_os = "windows") {
            normalized.to_lowercase()
        } else {
            normalized
        }
    }

    /// Generates a unique worktree name + directory under the data dir, and the
    /// `opencode/{name}` branch (unless detached). Mirrors `Worktree.candidate`.
    async fn candidate(&self, root: &str, name: Option<&str>, detached: bool, ctx: &InstanceContext) -> Result<WorktreeInfo, WorktreeError> {
        for attempt in 0..MAX_NAME_ATTEMPTS {
            let name = match name {
                Some(name) if !name.is_empty() => {
                    if attempt == 0 {
                        name.to_string()
                    } else {
                        format!("{name}-{}", Slug::create())
                    }
                }
                _ => Slug::create(),
            };
            let branch = if detached { None } else { Some(format!("opencode/{name}")) };
            let directory = pathutil::join(&[root, &name]);

            if fs::exists(&directory).await {
                continue;
            }
            if let Some(branch) = &branch {
                let r#ref = format!("refs/heads/{branch}");
                let branch_check = self.git(&["show-ref", "--verify", "--quiet", &r#ref], Some(&ctx.worktree)).await;
                if branch_check.code == 0 {
                    continue;
                }
            }
            return Ok(WorktreeInfo { name, branch, directory });
        }
        Err(WorktreeError::name_generation_failed("Failed to generate a unique worktree name"))
    }

    pub async fn make_worktree_info(
        &self,
        ctx: &InstanceContext,
        options: &WorktreeInfoOptions,
    ) -> Result<WorktreeInfo, WorktreeError> {
        if ctx.project.vcs.as_deref() != Some("git") {
            return Err(WorktreeError::not_git("Worktrees are only supported for git projects"));
        }
        let root = pathutil::join(&[&Global::paths().data.to_string_lossy(), "worktree", &ctx.project.id.0]);
        let _ = fs::make_dir_recursive(&root).await;
        let name = options.name.as_ref().map(|name| slugify(name)).unwrap_or_default();
        self.candidate(&root, if name.is_empty() { None } else { Some(&name) }, options.detached, ctx).await
    }

    async fn setup(&self, ctx: &InstanceContext, info: &WorktreeInfo) -> Result<(), WorktreeError> {
        let created = if let Some(branch) = &info.branch {
            self.git(&["worktree", "add", "--no-checkout", "-b", branch, &info.directory], Some(&ctx.worktree)).await
        } else {
            self.git(&["worktree", "add", "--no-checkout", "--detach", &info.directory, "HEAD"], Some(&ctx.worktree)).await
        };
        if created.code != 0 {
            let message = fallback(&created.stderr, &created.text, "Failed to create git worktree");
            return Err(WorktreeError::create_failed(message));
        }
        let _ = self.project.add_sandbox(&ctx.project.id, &info.directory).await;
        Ok(())
    }

    async fn boot(&self, ctx: &InstanceContext, info: &WorktreeInfo, start_command: Option<&str>) {
        let workspace_id = std::env::var("OPENCODE_WORKSPACE_ID").ok();
        let project_id = ctx.project.id.clone();
        let extra = start_command.map(|cmd| cmd.trim().to_string()).unwrap_or_default();

        let populated = self.git(&["reset", "--hard"], Some(&info.directory)).await;
        if populated.code != 0 {
            let message = fallback(&populated.stderr, &populated.text, "Failed to populate worktree");
            tracing::error!("worktree checkout failed: {}", message);
            self.emit_worktree_event(&info.directory, &project_id.0, workspace_id.as_deref(), "worktree.failed", &serde_json::json!({ "message": message }));
            return;
        }

        let booted = match self.store.load(crate::project::instance_store::LoadInput::directory(&info.directory)).await {
            Ok(_) => true,
            Err(error) => {
                let message = error.to_string();
                tracing::error!("worktree bootstrap failed: {}", message);
                self.emit_worktree_event(&info.directory, &project_id.0, workspace_id.as_deref(), "worktree.failed", &serde_json::json!({ "message": message }));
                false
            }
        };
        if !booted {
            return;
        }

        let mut properties = serde_json::json!({ "name": info.name });
        if let Some(branch) = &info.branch {
            properties["branch"] = serde_json::json!(branch);
        }
        self.emit_worktree_event(&info.directory, &project_id.0, workspace_id.as_deref(), "worktree.ready", &properties);

        self.run_start_scripts(&info.directory, &project_id.0, &extra).await;
    }

    fn emit_worktree_event(&self, directory: &str, project_id: &str, workspace_id: Option<&str>, r#type: &str, properties: &serde_json::Value) {
        self.bus.emit(BusEvent {
            directory: directory.to_string(),
            project: Some(project_id.to_string()),
            workspace: workspace_id.map(String::from),
            payload: EventPayload {
                r#type: r#type.to_string(),
                properties: Some(properties.clone()),
                data: None,
                location: None,
            },
        });
    }

    pub async fn create_from_info(&self, ctx: &InstanceContext, info: &WorktreeInfo, start_command: Option<&str>) -> Result<(), WorktreeError> {
        self.setup(ctx, info).await?;
        let worktree = self.clone();
        let ctx = ctx.clone();
        let info = info.clone();
        let start_command = start_command.map(String::from);
        tokio::spawn(async move {
            let _ = worktree.boot(&ctx, &info, start_command.as_deref()).await;
        });
        Ok(())
    }

    pub async fn create(&self, ctx: &InstanceContext, input: Option<&WorktreeCreateInput>) -> Result<WorktreeInfo, WorktreeError> {
        let info = self.make_worktree_info(ctx, &WorktreeInfoOptions { name: input.and_then(|i| i.name.clone()), detached: false }).await?;
        self.create_from_info(ctx, &info, input.and_then(|i| i.startCommand.clone()).as_deref()).await?;
        Ok(info)
    }

    pub async fn list(&self, ctx: &InstanceContext) -> Result<Vec<WorktreeInfo>, WorktreeError> {
        if ctx.project.vcs.as_deref() != Some("git") {
            return Ok(Vec::new());
        }
        let result = self.git(&["worktree", "list", "--porcelain"], Some(&ctx.worktree)).await;
        if result.code != 0 {
            let message = fallback(&result.stderr, &result.text, "Failed to read git worktrees");
            return Err(WorktreeError::list_failed(message));
        }

        let primary = self.canonical(&ctx.project.worktree).await;
        let primary_name = pathutil::basename(&primary).to_lowercase();
        let mut out = Vec::new();
        for entry in parse_worktree_list(&result.text) {
            let Some(path) = entry.path else { continue };
            let directory = self.canonical(&path).await;
            if directory == primary {
                continue;
            }
            let name = pathutil::basename(&directory).to_lowercase();
            let name = if name == primary_name { pathutil::basename(&pathutil::dirname(&directory)) } else { name };
            out.push(WorktreeInfo {
                name,
                directory,
                branch: entry.branch.map(|branch| branch.strip_prefix("refs/heads/").unwrap_or(&branch).to_string()),
            });
        }
        Ok(out)
    }

    async fn stop_fsmonitor(&self, target: &str) {
        if fs::exists(target).await {
            let _ = self.git(&["fsmonitor--daemon", "stop"], Some(target)).await;
        }
    }

    async fn clean_directory(&self, target: &str) -> Result<(), WorktreeError> {
        let attempts = if cfg!(target_os = "windows") { 50 } else { 5 };
        for attempt in 0..attempts {
            fs::remove_recursive(target).await;
            if !fs::exists(target).await {
                return Ok(());
            }
            if attempt == attempts - 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if fs::exists(target).await {
            Err(WorktreeError::remove_failed("Failed to remove git worktree directory"))
        } else {
            Ok(())
        }
    }

    pub async fn remove(&self, ctx: &InstanceContext, input: &WorktreeRemoveInput) -> Result<bool, WorktreeError> {
        if ctx.project.vcs.as_deref() != Some("git") {
            return Err(WorktreeError::not_git("Worktrees are only supported for git projects"));
        }
        let directory = self.canonical(&input.directory).await;

        // Preserve the loaded path casing for the store cache; `directory` is lowercased on Windows.
        if directory != self.canonical(&ctx.worktree).await {
            self.store.dispose_directory(&input.directory).await;
        }

        let list = self.git(&["worktree", "list", "--porcelain"], Some(&ctx.worktree)).await;
        if list.code != 0 {
            let message = fallback(&list.stderr, &list.text, "Failed to read git worktrees");
            return Err(WorktreeError::remove_failed(message));
        }

        let entries = parse_worktree_list(&list.text);
        let entry = self.locate_worktree(&entries, &directory).await;

        let Some(entry) = entry else {
            let directory_exists = fs::exists(&directory).await;
            if directory_exists {
                self.stop_fsmonitor(&directory).await;
                self.clean_directory(&directory).await?;
            }
            return Ok(true);
        };
        let Some(entry_path) = entry.path.clone() else {
            return Ok(true);
        };

        self.store.dispose_directory(&entry_path).await;
        self.stop_fsmonitor(&entry_path).await;
        let removed = self.git(&["worktree", "remove", "--force", &entry_path], Some(&ctx.worktree)).await;
        if removed.code != 0 {
            let next = self.git(&["worktree", "list", "--porcelain"], Some(&ctx.worktree)).await;
            if next.code != 0 {
                let message = fallback(&removed.stderr, &removed.text, "Failed to remove git worktree");
                return Err(WorktreeError::remove_failed(message));
            }
            let stale = self.locate_worktree(&parse_worktree_list(&next.text), &directory).await;
            if stale.is_some_and(|stale| stale.path.is_some()) {
                let message = fallback(&removed.stderr, &removed.text, "Failed to remove git worktree");
                return Err(WorktreeError::remove_failed(message));
            }
        }

        self.clean_directory(&entry_path).await?;

        if let Some(branch) = entry.branch.as_deref().map(|branch| branch.strip_prefix("refs/heads/").unwrap_or(branch)) {
            let deleted = self.git(&["branch", "-D", branch], Some(&ctx.worktree)).await;
            if deleted.code != 0 {
                let message = fallback(&deleted.stderr, &deleted.text, "Failed to delete worktree branch");
                return Err(WorktreeError::remove_failed(message));
            }
        }

        Ok(true)
    }

    async fn locate_worktree(&self, entries: &[WorktreeEntry], directory: &str) -> Option<WorktreeEntry> {
        for item in entries {
            if let Some(path) = &item.path {
                let key = self.canonical(path).await;
                if key == directory {
                    return Some(item.clone());
                }
            }
        }
        None
    }

    async fn git_expect(
        &self,
        args: &[&str],
        cwd: &str,
        error: impl Fn(&GitResult) -> WorktreeError,
    ) -> Result<GitResult, WorktreeError> {
        let result = self.git(args, Some(cwd)).await;
        if result.code != 0 {
            return Err(error(&result));
        }
        Ok(result)
    }

    async fn run_start_command(&self, directory: &str, cmd: &str) -> (i32, String) {
        let (shell, args): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
            ("cmd", vec!["/c", cmd])
        } else {
            ("bash", vec!["-lc", cmd])
        };
        let result = process::run(shell, &args, SpawnOptions { cwd: Some(directory.to_string()), ..Default::default() }).await;
        match result {
            Ok(result) => (result.exit_code, result.stderr_text()),
            Err(_) => (1, String::new()),
        }
    }

    async fn run_start_script(&self, directory: &str, cmd: &str, kind: &str) -> bool {
        let text = cmd.trim();
        if text.is_empty() {
            return true;
        }
        let (code, stderr) = self.run_start_command(directory, text).await;
        if code == 0 {
            return true;
        }
        tracing::error!("worktree start command failed: kind={kind} directory={directory} message={stderr}");
        false
    }

    async fn run_start_scripts(&self, directory: &str, project_id: &str, extra: &str) -> bool {
        let row = self.project.store.get_project(project_id);
        let startup = row
            .and_then(|row| row.commands)
            .and_then(|commands| commands.start)
            .map(|start| start.trim().to_string())
            .unwrap_or_default();
        let ok = self.run_start_script(directory, &startup, "project").await;
        if !ok {
            return false;
        }
        self.run_start_script(directory, extra, "worktree").await;
        true
    }

    async fn prune(&self, root: &str, entries: &[String]) {
        let base = self.canonical(root).await;
        for entry in entries {
            let target = self.canonical(&pathutil::join(&[root, entry])).await;
            if target == base {
                continue;
            }
            if !target.starts_with(&format!("{base}/")) {
                continue;
            }
            fs::remove_recursive(&target).await;
        }
    }

    async fn sweep(&self, root: &str) -> GitResult {
        let first = self.git(&["clean", "-ffdx"], Some(root)).await;
        if first.code == 0 {
            return first;
        }
        let entries = failed_removes(&[first.stderr.clone(), first.text.clone()]);
        if entries.is_empty() {
            return first;
        }
        self.prune(root, &entries).await;
        self.git(&["clean", "-ffdx"], Some(root)).await
    }

    pub async fn reset(&self, ctx: &InstanceContext, input: &WorktreeResetInput) -> Result<bool, WorktreeError> {
        if ctx.project.vcs.as_deref() != Some("git") {
            return Err(WorktreeError::not_git("Worktrees are only supported for git projects"));
        }
        let directory = self.canonical(&input.directory).await;
        let primary = self.canonical(&ctx.worktree).await;
        if directory == primary {
            return Err(WorktreeError::reset_failed("Cannot reset the primary workspace"));
        }

        let list = self.git(&["worktree", "list", "--porcelain"], Some(&ctx.worktree)).await;
        if list.code != 0 {
            let message = fallback(&list.stderr, &list.text, "Failed to read git worktrees");
            return Err(WorktreeError::reset_failed(message));
        }
        let entry = self.locate_worktree(&parse_worktree_list(&list.text), &directory).await;
        let Some(entry) = entry else {
            return Err(WorktreeError::reset_failed("Worktree not found"));
        };
        let worktree_path = entry.path.ok_or_else(|| WorktreeError::reset_failed("Worktree not found"))?;

        let base = self.git.default_branch(&ctx.worktree).await;
        let Some(base) = base else {
            return Err(WorktreeError::reset_failed("Default branch not found"));
        };

        if let Some(separator) = base.r#ref.find('/') {
            if base.r#ref != base.name {
                let remote = &base.r#ref[..separator];
                let branch = &base.r#ref[separator + 1..];
                self.git_expect(
                    &["fetch", remote, branch],
                    &ctx.worktree,
                    |result| WorktreeError::reset_failed(fallback(&result.stderr, &result.text, &format!("Failed to fetch {}", base.r#ref))),
                )
                .await?;
            }
        }

        self.git_expect(
            &["reset", "--hard", &base.r#ref],
            &worktree_path,
            |result| WorktreeError::reset_failed(fallback(&result.stderr, &result.text, "Failed to reset worktree to target")),
        )
        .await?;

        let clean_result = self.sweep(&worktree_path).await;
        if clean_result.code != 0 {
            let message = fallback(&clean_result.stderr, &clean_result.text, "Failed to clean worktree");
            return Err(WorktreeError::reset_failed(message));
        }

        self.git_expect(
            &["submodule", "update", "--init", "--recursive", "--force"],
            &worktree_path,
            |result| WorktreeError::reset_failed(fallback(&result.stderr, &result.text, "Failed to update submodules")),
        )
        .await?;
        self.git_expect(
            &["submodule", "foreach", "--recursive", "git", "reset", "--hard"],
            &worktree_path,
            |result| WorktreeError::reset_failed(fallback(&result.stderr, &result.text, "Failed to reset submodules")),
        )
        .await?;
        self.git_expect(
            &["submodule", "foreach", "--recursive", "git", "clean", "-fdx"],
            &worktree_path,
            |result| WorktreeError::reset_failed(fallback(&result.stderr, &result.text, "Failed to clean submodules")),
        )
        .await?;

        let status = self.git(&["-c", "core.fsmonitor=false", "status", "--porcelain=v1"], Some(&worktree_path)).await;
        if status.code != 0 {
            let message = fallback(&status.stderr, &status.text, "Failed to read git status");
            return Err(WorktreeError::reset_failed(message));
        }
        if !status.text.trim().is_empty() {
            let message = format!("Worktree reset left local changes:\n{}", status.text.trim());
            return Err(WorktreeError::reset_failed(message));
        }

        let worktree = self.clone();
        let project_id = ctx.project.id.0.clone();
        tokio::spawn(async move {
            let _ = worktree.run_start_scripts(&worktree_path, &project_id, "").await;
        });

        Ok(true)
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorktreeInfoOptions {
    pub name: Option<String>,
    pub detached: bool,
}

fn fallback(stderr: &str, text: &str, default: &str) -> String {
    let value = stderr.trim();
    let value = if value.is_empty() { text.trim() } else { value };
    if value.is_empty() {
        default.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_matches_reference() {
        assert_eq!(slugify("My Cool Project"), "my-cool-project");
        assert_eq!(slugify("  hello  world  "), "hello-world");
        assert_eq!(slugify("!!!"), "");
        assert_eq!(slugify("a--b"), "a-b");
    }

    #[test]
    fn failed_removes_parses_warnings() {
        let chunks = vec![
            "warning: failed to remove x.ts: Permission denied\n".to_string(),
            "other output".to_string(),
            "warning: failed to remove 'dir/y.ts': No such file\n".to_string(),
        ];
        let result = failed_removes(&chunks);
        assert_eq!(result, vec!["x.ts".to_string(), "dir/y.ts".to_string()]);
    }

    #[test]
    fn parse_worktree_list_parses_porcelain() {
        let text = "worktree /main\nHEAD 1234abcd\nbranch refs/heads/main\n\nworktree /data/wt/proj/feature\nHEAD abcd1234\nbranch refs/heads/opencode/feature\n";
        let entries = parse_worktree_list(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path.as_deref(), Some("/main"));
        assert_eq!(entries[0].branch.as_deref(), Some("refs/heads/main"));
        assert_eq!(entries[1].path.as_deref(), Some("/data/wt/proj/feature"));
        assert_eq!(entries[1].branch.as_deref(), Some("refs/heads/opencode/feature"));
    }

    #[test]
    fn worktree_name_path_rule() {
        // Golden: root = {data}/worktree/{projectID}, directory = join(root, name),
        // branch = opencode/{name}.
        let project_id = "pid";
        let name = "fix-thing";
        let root = pathutil::join(&["/data", "worktree", project_id]);
        let directory = pathutil::join(&[&root, name]);
        let branch = format!("opencode/{name}");
        assert_eq!(root, "/data/worktree/pid");
        assert_eq!(directory, "/data/worktree/pid/fix-thing");
        assert_eq!(branch, "opencode/fix-thing");
    }
}
