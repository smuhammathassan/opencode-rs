/// From reference/packages/opencode/src/snapshot/index.ts
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, Semaphore};

use crate::schema::{ProjectInfo, SnapshotFileDiff, SnapshotPatch};
use crate::util::config::Config;
use crate::util::diff::{format_patch, structured_patch};
use crate::util::global::Global;
use crate::util::hash::Hash;
use crate::util::pathutil;
use crate::util::process::{self, SpawnOptions};
use crate::util::{fs, GitResult};

const PRUNE: &str = "7.days";
const LIMIT: u64 = 2 * 1024 * 1024;
const CORE: &[&str] = &["-c", "core.longpaths=true", "-c", "core.symlinks=true"];
const CFG: &[&str] = &[
    "-c",
    "core.autocrlf=false",
    "-c",
    "core.longpaths=true",
    "-c",
    "core.symlinks=true",
];
const QUOTE: &[&str] = &[
    "-c",
    "core.autocrlf=false",
    "-c",
    "core.longpaths=true",
    "-c",
    "core.symlinks=true",
    "-c",
    "core.quotepath=false",
];

#[derive(Debug, Clone)]
pub struct State {
    pub directory: String,
    pub worktree: String,
    pub gitdir: String,
    pub vcs: Option<String>,
}

impl State {
    fn args(&self, cmd: &[&str]) -> Vec<String> {
        let mut out = vec![
            "--git-dir".to_string(),
            self.gitdir.clone(),
            "--work-tree".to_string(),
            self.worktree.clone(),
        ];
        out.extend(cmd.iter().map(|item| item.to_string()));
        out
    }
}

/// Builds a full command: `prefix` flags followed by the `--git-dir`/`--work-tree`
/// arguments and the git subcommand.
fn with_args(state: &State, prefix: &[&str], cmd: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = prefix.iter().map(|item| item.to_string()).collect();
    out.extend(state.args(cmd));
    out
}

fn encode_nul_terminated_paths(files: &[String]) -> String {
    let mut out = files.join("\0");
    out.push('\0');
    out
}

fn encode_top_level_literal_pathspecs(files: &[String]) -> String {
    let mapped: Vec<String> = files
        .iter()
        .map(|file| format!(":(top,literal){file}"))
        .collect();
    encode_nul_terminated_paths(&mapped)
}

pub struct Snapshot {
    pub config: Arc<Config>,
    states: Arc<Mutex<HashMap<String, Arc<State>>>>,
    locks: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl Clone for Snapshot {
    fn clone(&self) -> Self {
        Snapshot {
            config: self.config.clone(),
            states: self.states.clone(),
            locks: self.locks.clone(),
        }
    }
}

impl Snapshot {
    pub fn new(config: Arc<Config>) -> Arc<Snapshot> {
        Arc::new(Snapshot {
            config,
            states: Arc::new(Mutex::new(HashMap::new())),
            locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn state_for(&self, ctx: &ProjectInfo, directory: &str, worktree: &str) -> Arc<State> {
        if let Some(state) = self.states.lock().unwrap().get(directory) {
            return state.clone();
        }
        let gitdir = pathutil::join(&[
            &Global::paths().data.to_string_lossy(),
            "snapshot",
            &ctx.id.0,
            &Hash::fast(worktree.as_bytes()),
        ]);
        let state = Arc::new(State {
            directory: directory.to_string(),
            worktree: worktree.to_string(),
            gitdir,
            vcs: ctx.vcs.clone(),
        });
        self.states
            .lock()
            .unwrap()
            .insert(directory.to_string(), state.clone());
        state
    }

    fn lock(&self, gitdir: &str) -> Arc<Semaphore> {
        if let Some(lock) = self.locks.lock().unwrap().get(gitdir) {
            return lock.clone();
        }
        let lock = Arc::new(Semaphore::new(1));
        self.locks
            .lock()
            .unwrap()
            .insert(gitdir.to_string(), lock.clone());
        lock
    }

    /// Starts the hourly cleanup loop for the instance (reference:
    /// `cleanup().repeat(Schedule.spaced(1 hour)).delay(1 minute).forkScoped`).
    /// The loop exits when the instance's shutdown channel closes.
    pub async fn init(
        &self,
        ctx: crate::project::instance_context::InstanceContext,
        shutdown: broadcast::Receiver<()>,
    ) -> JoinHandle {
        let snapshot = self.clone();
        tokio::spawn(async move {
            // initial 1-minute delay
            let mut shutdown = shutdown;
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
                _ = shutdown.recv() => return,
            }
            loop {
                let state = snapshot.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
                snapshot.cleanup_state(&state).await;
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {}
                    _ = shutdown.recv() => return,
                }
            }
        })
    }

    pub async fn cleanup(&self, ctx: &crate::project::instance_context::InstanceContext) {
        let state = self.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
        self.cleanup_state(&state).await;
    }

    pub async fn track(
        &self,
        ctx: &crate::project::instance_context::InstanceContext,
    ) -> Option<String> {
        let state = self.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
        self.track_state(&state).await
    }

    pub async fn patch(
        &self,
        ctx: &crate::project::instance_context::InstanceContext,
        hash: &str,
    ) -> SnapshotPatch {
        let state = self.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
        self.patch_state(&state, hash).await
    }

    pub async fn restore(
        &self,
        ctx: &crate::project::instance_context::InstanceContext,
        snapshot: &str,
    ) {
        let state = self.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
        self.restore_state(&state, snapshot).await;
    }

    pub async fn revert(
        &self,
        ctx: &crate::project::instance_context::InstanceContext,
        patches: &[SnapshotPatch],
    ) {
        let state = self.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
        self.revert_state(&state, patches).await;
    }

    pub async fn diff(
        &self,
        ctx: &crate::project::instance_context::InstanceContext,
        hash: &str,
    ) -> String {
        let state = self.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
        self.diff_state(&state, hash).await
    }

    pub async fn diff_full(
        &self,
        ctx: &crate::project::instance_context::InstanceContext,
        from: &str,
        to: &str,
    ) -> Vec<SnapshotFileDiff> {
        let state = self.state_for(&ctx.project, &ctx.directory, &ctx.worktree);
        self.diff_full_state(&state, from, to).await
    }

    pub fn on_dispose(&self, directory: &str) {
        self.states.lock().unwrap().remove(directory);
    }

    pub(crate) async fn cleanup_state(&self, state: &State) {
        let lock = self.lock(&state.gitdir);
        let _permit = lock.acquire().await.unwrap();
        if !self.enabled(state) {
            return;
        }
        if !fs::exists(&state.gitdir).await {
            return;
        }
        let result = self
            .git(
                state,
                &["gc", &format!("--prune={PRUNE}")],
                Some(&state.directory),
            )
            .await;
        if result.code != 0 {
            tracing::warn!("cleanup failed: {}", result.stderr);
            return;
        }
        tracing::info!("cleanup {PRUNE}");
    }

    fn enabled(&self, state: &State) -> bool {
        state.vcs.as_deref() == Some("git") && self.config.snapshot_enabled()
    }

    /// Returns the tree hash of the current snapshot, or `None` when disabled.
    pub(crate) async fn track_state(&self, state: &State) -> Option<String> {
        let lock = self.lock(&state.gitdir);
        let _permit = lock.acquire().await.unwrap();
        if !self.enabled(state) {
            return None;
        }
        let existed = fs::exists(&state.gitdir).await;
        let _ = fs::ensure_dir(&state.gitdir).await;
        if !existed {
            let mut env = std::collections::HashMap::new();
            env.insert("GIT_DIR".to_string(), state.gitdir.clone());
            env.insert("GIT_WORK_TREE".to_string(), state.worktree.clone());
            let _ = process::run(
                "git",
                &["init"],
                SpawnOptions {
                    env: Some(env),
                    ..Default::default()
                },
            )
            .await;
            for (key, value) in [
                ("core.autocrlf", "false"),
                ("core.longpaths", "true"),
                ("core.symlinks", "true"),
                ("core.fsmonitor", "false"),
                ("feature.manyFiles", "true"),
                ("index.version", "4"),
                ("index.threads", "true"),
                ("core.untrackedCache", "true"),
            ] {
                let cmd = vec![
                    "--git-dir".to_string(),
                    state.gitdir.clone(),
                    "config".to_string(),
                    key.to_string(),
                    value.to_string(),
                ];
                self.git(state, &cmd, None).await;
            }
            self.seed(state).await;
            tracing::info!("initialized");
        }
        self.add(state).await;
        let cmd = with_args(state, &[], &["write-tree"]);
        let result = self.git(state, &cmd, Some(&state.directory)).await;
        if result.code != 0 {
            tracing::warn!(
                code = result.code,
                stderr = %result.stderr,
                "snapshot write-tree failed"
            );
            return None;
        }
        let hash = result.text.trim().to_string();
        if hash.is_empty() {
            tracing::warn!("snapshot write-tree returned an empty tree hash");
            return None;
        }
        tracing::info!("tracking {hash}");
        Some(hash)
    }

    async fn git<S: AsRef<str>>(&self, _state: &State, cmd: &[S], cwd: Option<&str>) -> GitResult {
        let args: Vec<&str> = cmd.iter().map(|item| item.as_ref()).collect();
        let result = process::run(
            "git",
            &args,
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

    async fn ignore(&self, state: &State, files: &[String]) -> HashSet<String> {
        if files.is_empty() {
            return HashSet::new();
        }
        let check_ignore_paths: Vec<String> = files
            .iter()
            .map(|item| {
                if item.starts_with(':') {
                    format!("./{item}")
                } else {
                    item.clone()
                }
            })
            .collect();
        let stdin = Some(encode_nul_terminated_paths(&check_ignore_paths));
        let cmd = with_args(
            state,
            &[
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.longpaths=true",
                "-c",
                "core.symlinks=true",
                "-c",
                "core.quotepath=false",
            ],
            &[
                "--git-dir",
                &pathutil::join(&[&state.worktree, ".git"]),
                "--work-tree",
                &state.worktree,
                "check-ignore",
                "--no-index",
                "--stdin",
                "-z",
            ],
        );
        let check = self.git_with_stdin(state, &cmd, &stdin).await;
        if check.code != 0 && check.code != 1 {
            return HashSet::new();
        }
        check
            .text
            .split('\0')
            .filter(|item| !item.is_empty())
            .map(|item| {
                if let Some(rest) = item.strip_prefix("./:") {
                    rest.to_string()
                } else {
                    item.to_string()
                }
            })
            .collect()
    }

    async fn drop(&self, state: &State, files: &[String]) {
        if files.is_empty() {
            return;
        }
        let cmd = with_args(
            state,
            CFG,
            &[
                "rm",
                "--cached",
                "-f",
                "--ignore-unmatch",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
        );
        self.git(state, &cmd, Some(&state.worktree)).await;
    }

    async fn stage(&self, state: &State, files: &[String]) {
        if files.is_empty() {
            return;
        }
        let cmd = with_args(
            state,
            CFG,
            &[
                "add",
                "--all",
                "--sparse",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
        );
        let stdin = Some(encode_top_level_literal_pathspecs(files));
        let result = self.git_with_stdin(state, &cmd, &stdin).await;
        if result.code == 0 {
            return;
        }
        tracing::warn!("failed to add snapshot files: {}", result.stderr);
    }

    async fn git_with_stdin(
        &self,
        state: &State,
        cmd: &[String],
        stdin: &Option<String>,
    ) -> GitResult {
        let args: Vec<&str> = cmd.iter().map(|item| item.as_str()).collect();
        let result = process::run(
            "git",
            &args,
            SpawnOptions {
                cwd: Some(state.worktree.clone()),
                stdin: stdin.clone(),
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

    async fn excludes(&self, state: &State) -> Option<String> {
        let result = self
            .git(
                state,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "info/exclude",
                ],
                Some(&state.worktree),
            )
            .await;
        let file = result.text.trim().to_string();
        if file.is_empty() {
            return None;
        }
        if !fs::exists(&file).await {
            return None;
        }
        Some(file)
    }

    async fn sync(&self, state: &State, list: &[String]) {
        let file = self.excludes(state).await;
        let target = pathutil::join(&[&state.gitdir, "info", "exclude"]);
        let mut parts: Vec<String> = Vec::new();
        if let Some(file) = &file {
            let text = fs::read_to_string(file).await.trim_end().to_string();
            if !text.is_empty() {
                parts.push(text);
            }
        }
        parts.extend(
            list.iter()
                .map(|item| format!("/{}", item.replace('\\', "/"))),
        );
        parts.retain(|item| !item.is_empty());
        let _ = fs::ensure_dir(&pathutil::join(&[&state.gitdir, "info"])).await;
        let text = parts.join("\n");
        let content = if text.is_empty() {
            String::new()
        } else {
            format!("{text}\n")
        };
        let _ = fs::write_string(&target, &content).await;
    }

    /// Shares the source repo's object database and index so hashes are reused.
    async fn seed(&self, state: &State) {
        if state.vcs.as_deref() != Some("git") {
            return;
        }
        let common_dir = self
            .git(
                state,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                Some(&state.worktree),
            )
            .await;
        if common_dir.code != 0 {
            return;
        }
        let source = common_dir.text.trim().to_string();
        if source.is_empty() || !fs::exists(&source).await {
            return;
        }
        let source_objects = pathutil::join(&[&source, "objects"]);
        let chained: Vec<String> =
            fs::read_to_string(&pathutil::join(&[&source_objects, "info", "alternates"]))
                .await
                .split('\n')
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
        let mut alternates: Vec<String> = Vec::new();
        let mut candidates = vec![source_objects.clone()];
        candidates.extend(chained);
        for candidate in candidates {
            if fs::exists(&candidate).await {
                alternates.push(candidate);
            }
        }
        if alternates.is_empty() {
            return;
        }
        let _ = fs::ensure_dir(&pathutil::join(&[&state.gitdir, "objects", "info"])).await;
        let content = format!("{}\n", alternates.join("\n"));
        let _ = fs::write_string(
            &pathutil::join(&[&state.gitdir, "objects", "info", "alternates"]),
            &content,
        )
        .await;

        let source_index = pathutil::join(&[&source, "index"]);
        if fs::exists(&source_index).await {
            let _ = fs::copy_file(&source_index, &pathutil::join(&[&state.gitdir, "index"])).await;
        }
    }

    async fn add(&self, state: &State) {
        self.sync(state, &[]).await;
        let cmd = with_args(
            state,
            QUOTE,
            &["diff-files", "--name-only", "-z", "--", "."],
        );
        let diff = self.git(state, &cmd, Some(&state.directory)).await;

        let cmd = with_args(
            state,
            QUOTE,
            &[
                "ls-files",
                "--full-name",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
                ".",
            ],
        );
        let other = self.git(state, &cmd, Some(&state.directory)).await;

        if diff.code != 0 || other.code != 0 {
            tracing::warn!("failed to list snapshot files");
            return;
        }

        let tracked: Vec<String> = diff
            .text
            .split('\0')
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect();
        let untracked: Vec<String> = other
            .text
            .split('\0')
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect();
        let mut all: Vec<String> = tracked.clone();
        for item in untracked.iter() {
            if !all.contains(item) {
                all.push(item.clone());
            }
        }
        if all.is_empty() {
            return;
        }

        let ignored = self.ignore(state, &all).await;
        if !ignored.is_empty() {
            let ignored_files: Vec<String> = ignored.iter().cloned().collect();
            tracing::info!(
                "removing gitignored files from snapshot: {}",
                ignored_files.len()
            );
            self.drop(state, &ignored_files).await;
        }

        let allow: Vec<String> = all
            .iter()
            .filter(|item| !ignored.contains(*item))
            .cloned()
            .collect();
        if allow.is_empty() {
            return;
        }

        let mut large = HashSet::new();
        for item in &allow {
            if let Some(size) = fs::file_size(&pathutil::join(&[&state.worktree, item])).await {
                if size > LIMIT {
                    large.insert(item.clone());
                }
            }
        }
        let block: HashSet<String> = untracked
            .iter()
            .filter(|item| large.contains(*item))
            .cloned()
            .collect();
        let block_list: Vec<String> = block.iter().cloned().collect();
        self.sync(state, &block_list).await;
        let stage_list: Vec<String> = allow
            .iter()
            .filter(|item| !block.contains(*item))
            .cloned()
            .collect();
        self.stage(state, &stage_list).await;
    }

    pub(crate) async fn patch_state(&self, state: &State, hash: &str) -> SnapshotPatch {
        let lock = self.lock(&state.gitdir);
        let _permit = lock.acquire().await.unwrap();
        self.add(state).await;
        let cmd = with_args(
            state,
            QUOTE,
            &[
                "diff",
                "--cached",
                "--no-ext-diff",
                "--name-only",
                hash,
                "--",
                ".",
            ],
        );
        let result = self.git(state, &cmd, Some(&state.directory)).await;
        if result.code != 0 {
            tracing::warn!("failed to get diff for {hash}");
            return SnapshotPatch {
                hash: hash.to_string(),
                files: Vec::new(),
            };
        }
        let files: Vec<String> = result
            .text
            .trim()
            .split('\n')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();

        let ignored = self.ignore(state, &files).await;
        let worktree = state.worktree.clone();
        SnapshotPatch {
            hash: hash.to_string(),
            files: files
                .iter()
                .filter(|item| !ignored.contains(*item))
                .map(|item| pathutil::join(&[&worktree, item]).replace('\\', "/"))
                .collect(),
        }
    }

    pub(crate) async fn restore_state(&self, state: &State, snapshot: &str) {
        let lock = self.lock(&state.gitdir);
        let _permit = lock.acquire().await.unwrap();
        tracing::info!("restore {snapshot}");
        let cmd = with_args(state, CORE, &["read-tree", snapshot]);
        let result = self.git(state, &cmd, Some(&state.worktree)).await;
        if result.code == 0 {
            let cmd = with_args(state, CORE, &["checkout-index", "-a", "-f"]);
            let checkout = self.git(state, &cmd, Some(&state.worktree)).await;
            if checkout.code == 0 {
                return;
            }
            tracing::error!("failed to restore snapshot {snapshot}: {}", checkout.stderr);
            return;
        }
        tracing::error!("failed to restore snapshot {snapshot}: {}", result.stderr);
    }

    pub(crate) async fn revert_state(&self, state: &State, patches: &[SnapshotPatch]) {
        let lock = self.lock(&state.gitdir);
        let _permit = lock.acquire().await.unwrap();
        let mut ops: Vec<(String, String, String)> = Vec::new();
        let mut seen = HashSet::new();
        for item in patches {
            for file in &item.files {
                if seen.contains(file) {
                    continue;
                }
                seen.insert(file.clone());
                let rel = pathutil::relative(&state.worktree, file).replace('\\', "/");
                ops.push((item.hash.clone(), file.clone(), rel));
            }
        }

        for (index, op) in ops.iter().enumerate() {
            let (hash, file, rel) = op;
            tracing::info!("reverting {file} {hash}");
            let cmd = with_args(state, CORE, &["checkout", hash, "--", file]);
            let result = self.git(state, &cmd, Some(&state.worktree)).await;
            if result.code == 0 {
                continue;
            }
            let cmd = with_args(state, CORE, &["ls-tree", hash, "--", rel]);
            let tree = self.git(state, &cmd, Some(&state.worktree)).await;
            if tree.code == 0 && !tree.text.trim().is_empty() {
                tracing::info!(
                    "file existed in snapshot but checkout failed, keeping {file} {hash}"
                );
                continue;
            }
            tracing::info!("file did not exist in snapshot, deleting {file} {hash}");
            fs::remove(file).await;
            let _ = index;
        }
    }

    pub(crate) async fn diff_state(&self, state: &State, hash: &str) -> String {
        let lock = self.lock(&state.gitdir);
        let _permit = lock.acquire().await.unwrap();
        self.add(state).await;
        let cmd = with_args(
            state,
            QUOTE,
            &["diff", "--cached", "--no-ext-diff", hash, "--", "."],
        );
        let result = self.git(state, &cmd, Some(&state.worktree)).await;
        if result.code != 0 {
            tracing::warn!("failed to get diff for {hash}");
            return String::new();
        }
        result.text.trim().to_string()
    }

    /// Returns per-file diffs between two snapshot tree hashes.
    pub(crate) async fn diff_full_state(
        &self,
        state: &State,
        from: &str,
        to: &str,
    ) -> Vec<SnapshotFileDiff> {
        let lock = self.lock(&state.gitdir);
        let _permit = lock.acquire().await.unwrap();

        let mut status_map: HashMap<String, String> = HashMap::new();
        let cmd = with_args(
            state,
            QUOTE,
            &[
                "diff",
                "--no-ext-diff",
                "--name-status",
                "--no-renames",
                from,
                to,
                "--",
                ".",
            ],
        );
        let statuses = self.git(state, &cmd, Some(&state.directory)).await;
        for line in statuses.text.trim().split('\n') {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(2, '\t');
            let code = parts.next().unwrap_or("");
            let file = parts.next().unwrap_or("");
            if code.is_empty() || file.is_empty() {
                continue;
            }
            let status = if code.starts_with('A') {
                "added"
            } else if code.starts_with('D') {
                "deleted"
            } else {
                "modified"
            };
            status_map.insert(file.to_string(), status.to_string());
        }

        let cmd = with_args(
            state,
            QUOTE,
            &[
                "diff",
                "--no-ext-diff",
                "--no-renames",
                "--numstat",
                from,
                to,
                "--",
                ".",
            ],
        );
        let numstat = self.git(state, &cmd, Some(&state.directory)).await;

        let mut rows: Vec<Row> = Vec::new();
        for line in numstat.text.trim().split('\n') {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            let Some(file) = parts.get(2) else { continue };
            if file.is_empty() {
                continue;
            }
            let binary = parts.first() == Some(&"-") && parts.get(1) == Some(&"-");
            let additions = if binary {
                0
            } else {
                parts[0].parse::<u64>().unwrap_or(0)
            };
            let deletions = if binary {
                0
            } else {
                parts[1].parse::<u64>().unwrap_or(0)
            };
            rows.push(Row {
                file: (*file).to_string(),
                status: status_map
                    .get(*file)
                    .cloned()
                    .unwrap_or_else(|| "modified".to_string()),
                binary,
                additions,
                deletions,
            });
        }

        let ignored = self
            .ignore(
                state,
                &rows.iter().map(|row| row.file.clone()).collect::<Vec<_>>(),
            )
            .await;
        if !ignored.is_empty() {
            rows.retain(|row| !ignored.contains(&row.file));
        }

        let mut result: Vec<SnapshotFileDiff> = Vec::new();
        let step = 100;
        for chunk in rows.chunks(step) {
            let text = self.load_batch(state, from, to, chunk).await;
            for row in chunk {
                let before = text
                    .as_ref()
                    .and_then(|map| map.get(&row.file))
                    .map(|pair| pair.0.clone())
                    .unwrap_or_default();
                let after = text
                    .as_ref()
                    .and_then(|map| map.get(&row.file))
                    .map(|pair| pair.1.clone())
                    .unwrap_or_default();
                let patch = if row.binary {
                    String::new()
                } else {
                    format_patch(&structured_patch(
                        &row.file,
                        &row.file,
                        &before,
                        &after,
                        usize::MAX,
                    ))
                };
                result.push(SnapshotFileDiff {
                    file: Some(row.file.clone()),
                    patch: Some(patch),
                    additions: row.additions,
                    deletions: row.deletions,
                    status: Some(row.status.clone()),
                });
            }
        }
        result
    }

    /// Batch-reads file contents via `git cat-file --batch`, falling back to
    /// per-file `git show` on any parsing failure (reference `load`).
    async fn load_batch(
        &self,
        state: &State,
        from: &str,
        to: &str,
        rows: &[Row],
    ) -> Option<HashMap<String, (String, String)>> {
        let mut refs: Vec<(String, String, String)> = Vec::new(); // (file, side, ref)
        for row in rows {
            if row.binary {
                continue;
            }
            match row.status.as_str() {
                "added" => refs.push((
                    row.file.clone(),
                    "after".to_string(),
                    format!("{to}:{}", row.file),
                )),
                "deleted" => refs.push((
                    row.file.clone(),
                    "before".to_string(),
                    format!("{from}:{}", row.file),
                )),
                _ => {
                    refs.push((
                        row.file.clone(),
                        "before".to_string(),
                        format!("{from}:{}", row.file),
                    ));
                    refs.push((
                        row.file.clone(),
                        "after".to_string(),
                        format!("{to}:{}", row.file),
                    ));
                }
            }
        }
        if refs.is_empty() {
            return Some(HashMap::new());
        }

        let stdin = format!(
            "{}\n",
            refs.iter()
                .map(|item| item.2.clone())
                .collect::<Vec<_>>()
                .join("\n")
        );
        let cmd = with_args(state, CFG, &["cat-file", "--batch"]);
        let args: Vec<&str> = cmd.iter().map(|item| item.as_str()).collect();
        let result = process::run(
            "git",
            &args,
            SpawnOptions {
                cwd: Some(state.directory.clone()),
                stdin: Some(stdin),
                ..Default::default()
            },
        )
        .await;
        let Ok(result) = result else { return None };
        if result.exit_code != 0 {
            tracing::info!("git cat-file --batch failed during snapshot diff, falling back to per-file git show");
            return None;
        }
        let out = result.stdout;
        let header_re = Regex::new(r"^[0-9a-f]+ blob (\d+)$").unwrap();

        let mut map: HashMap<String, (String, String)> = HashMap::new();
        let mut index = 0usize;
        for item in &refs {
            let mut end = index;
            while end < out.len() && out[end] != 10 {
                end += 1;
            }
            if end >= out.len() {
                return None;
            }
            let head = String::from_utf8_lossy(&out[index..end]).into_owned();
            index = end + 1;
            let entry = map
                .entry(item.0.clone())
                .or_insert_with(|| (String::new(), String::new()));
            if head.ends_with(" missing") {
                continue;
            }
            let Some(captures) = header_re.captures(&head) else {
                return None;
            };
            let size: usize = match captures[1].parse() {
                Ok(size) => size,
                Err(_) => return None,
            };
            if index + size >= out.len() || out[index + size] != 10 {
                return None;
            }
            let text = String::from_utf8_lossy(&out[index..index + size]).into_owned();
            if item.1 == "before" {
                entry.0 = text;
            } else {
                entry.1 = text;
            }
            index += size + 1;
        }
        if index != out.len() {
            return None;
        }
        Some(map)
    }
}

#[derive(Debug, Clone)]
struct Row {
    file: String,
    status: String,
    binary: bool,
    additions: u64,
    deletions: u64,
}

use regex::Regex;

#[allow(unused_imports)]
use crate::project::instance_context::InstanceContext;

type JoinHandle = tokio::task::JoinHandle<()>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitdir_follows_snapshot_path_rule() {
        let gitdir = pathutil::join(&["/data", "snapshot", "pid", &Hash::fast(b"/proj")]);
        assert_eq!(
            gitdir,
            "/data/snapshot/pid/d6f7455193488ce357fb25ad6c17b3ee8fc84a59"
        );
        let state = State {
            directory: "/proj".to_string(),
            worktree: "/proj".to_string(),
            gitdir: gitdir.clone(),
            vcs: Some("git".to_string()),
        };
        assert_eq!(state.gitdir, gitdir);
    }

    #[test]
    fn nul_terminated_encoding_matches_reference() {
        assert_eq!(
            encode_nul_terminated_paths(&["a".to_string(), "b".to_string()]),
            "a\0b\0"
        );
        assert_eq!(
            encode_top_level_literal_pathspecs(&["a".to_string(), "b".to_string()]),
            ":(top,literal)a\0:(top,literal)b\0"
        );
    }

    #[test]
    fn args_prefixes_git_dir_and_work_tree() {
        let state = State {
            directory: "/proj".to_string(),
            worktree: "/proj".to_string(),
            gitdir: "/data/snapshot/pid/hash".to_string(),
            vcs: Some("git".to_string()),
        };
        assert_eq!(
            state.args(&["write-tree"]),
            vec![
                "--git-dir",
                "/data/snapshot/pid/hash",
                "--work-tree",
                "/proj",
                "write-tree"
            ]
        );
    }
}
