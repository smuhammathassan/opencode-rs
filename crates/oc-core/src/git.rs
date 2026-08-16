//! Git wrapper service.
//!
//! From reference/packages/core/src/git.ts — the `Git.Service` implementation.
//! All commands run `git` via [`crate::process`] with the repository's
//! `--git-dir`/`--work-tree` arguments and a per-repository keyed lock for
//! stateful tree operations.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file::FileDiff;
use crate::fs_util::FSUtilService;
use crate::keyed_mutex::KeyedMutex;
use crate::process::{self, Command, RunOptions, Stdin};
use crate::schema::{trim_newlines, AbsolutePath, RelativePath};

/// `Git.Repository` — `{ worktree, gitDirectory, commonDirectory }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub worktree: AbsolutePath,
    pub gitDirectory: AbsolutePath,
    pub commonDirectory: AbsolutePath,
}

/// `Git.ChangeSet` — branded string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChangeSet(pub String);

/// `Git.TreeID` — branded string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TreeId(pub String);

/// `Git.Worktree` — `{ directory, kind }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktree {
    pub directory: AbsolutePath,
    pub kind: String,
}

/// `Git.OperationError` — `_tag: "Git.OperationError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationError {
    pub _tag: String,
    pub operation: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<AbsolutePath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

impl OperationError {
    fn new(operation: &str, message: impl Into<String>) -> Self {
        OperationError {
            _tag: "Git.OperationError".to_string(),
            operation: operation.to_string(),
            message: message.into(),
            directory: None,
            cause: None,
        }
    }
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Git {} failed: {}", self.operation, self.message)
    }
}

impl std::error::Error for OperationError {}

/// `Git.WorktreeError` — `_tag: "Git.WorktreeError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeError {
    pub _tag: String,
    pub operation: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<AbsolutePath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// `Git.PatchError` — `_tag: "Git.PatchError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchError {
    pub _tag: String,
    pub operation: String,
    pub directory: AbsolutePath,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

impl PatchError {
    fn new(operation: &str, directory: &AbsolutePath, message: impl Into<String>) -> Self {
        PatchError {
            _tag: "Git.PatchError".to_string(),
            operation: operation.to_string(),
            directory: directory.clone(),
            message: message.into(),
            cause: None,
        }
    }
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Git {} failed: {}", self.operation, self.message)
    }
}

impl std::error::Error for PatchError {}

impl std::fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Git {} failed: {}", self.operation, self.message)
    }
}

impl std::error::Error for WorktreeError {}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error(transparent)]
    Operation(#[from] OperationError),
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
    #[error(transparent)]
    Patch(#[from] PatchError),
}

#[derive(Debug, Default, Clone)]
pub struct OperationOptions {
    pub stdin: Option<Vec<u8>>,
    pub env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
struct ExecResult {
    exit_code: i32,
    text: String,
    stderr: String,
}

/// The git service (`@opencode/GitV2`).
#[derive(Clone)]
pub struct GitService {
    fs: Arc<FSUtilService>,
    locks: Arc<KeyedMutex<String>>,
}

impl GitService {
    pub fn new(fs: Arc<FSUtilService>) -> Self {
        GitService {
            fs,
            locks: Arc::new(KeyedMutex::make()),
        }
    }

    // repo ------------------------------------------------------------------

    /// `repo.discover(input)`.
    pub async fn discover(&self, input: &str) -> Option<Repository> {
        let matches = self.fs.up(&[".git"], input, None).await.unwrap_or_default();
        let dotgit = matches.first()?.clone();
        let cwd = Path::new(&dotgit)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let top_level = self.run(&cwd, &["rev-parse", "--show-toplevel"]).await;
        let git_dir = self.run(&cwd, &["rev-parse", "--git-dir"]).await;
        let common_dir = self.run(&cwd, &["rev-parse", "--git-common-dir"]).await;
        if git_dir.exit_code != 0 || common_dir.exit_code != 0 {
            return None;
        }
        let worktree = if top_level.exit_code == 0 {
            resolve_path(&cwd, &top_level.text)
        } else {
            cwd.clone()
        };
        Some(Repository {
            worktree: AbsolutePath(worktree),
            gitDirectory: AbsolutePath(resolve_path(&cwd, &git_dir.text)),
            commonDirectory: AbsolutePath(resolve_path(&cwd, &common_dir.text)),
        })
    }

    /// `repo.clone(input)`.
    pub async fn repo_clone(
        &self,
        remote: &str,
        directory: &AbsolutePath,
        branch: Option<&str>,
        depth: Option<u32>,
    ) -> Result<Repository, OperationError> {
        let parent = Path::new(&directory.0)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut args: Vec<String> = vec![
            "clone".into(),
            "--depth".into(),
            (depth.unwrap_or(100)).to_string(),
        ];
        if let Some(branch) = branch {
            args.push("--branch".into());
            args.push(branch.to_string());
        }
        args.push("--".into());
        args.push(remote.to_string());
        args.push(directory.0.clone());
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.operation("clone", &AbsolutePath(parent), &refs)
            .await?;
        match self.discover(&directory.0).await {
            Some(repository) => Ok(repository),
            None => Err(OperationError::new(
                "clone",
                format!("Cloned repository could not be opened: {}", directory.0),
            )),
        }
    }

    /// `repo.create(input)`.
    pub async fn repo_create(
        &self,
        worktree: &AbsolutePath,
        git_directory: &AbsolutePath,
        seed: Option<&Repository>,
    ) -> Result<Repository, OperationError> {
        self.fs
            .ensure_dir(&git_directory.0)
            .await
            .map_err(|_| OperationError::new("create", "Failed to create Git storage"))?;
        let repository = Repository {
            worktree: worktree.clone(),
            gitDirectory: git_directory.clone(),
            commonDirectory: git_directory.clone(),
        };
        self.repository_operation("create", &repository, &["init"])
            .await?;
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
            self.repository_operation("create", &repository, &["config", key, value])
                .await?;
        }
        let Some(seed) = seed else {
            return Ok(repository);
        };
        let info_dir = Path::new(&git_directory.0).join("objects").join("info");
        self.fs
            .ensure_dir(&info_dir.display().to_string())
            .await
            .map_err(|_| OperationError::new("create", "Failed to configure shared Git objects"))?;
        let alternates = format!("{}/objects\n", seed.commonDirectory.0);
        self.fs
            .write_file_string(
                &info_dir.join("alternates").display().to_string(),
                &alternates,
            )
            .await
            .map_err(|_| OperationError::new("create", "Failed to configure shared Git objects"))?;
        let index_from = Path::new(&seed.gitDirectory.0).join("index");
        let index_to = Path::new(&git_directory.0).join("index");
        let _ = self
            .fs
            .copy_file(
                &index_from.display().to_string(),
                &index_to.display().to_string(),
            )
            .await;
        Ok(repository)
    }

    // remote ----------------------------------------------------------------

    /// `remote.get(repository, name)`.
    pub async fn remote_get(&self, repository: &Repository, name: &str) -> Option<String> {
        let result = self
            .run(&repository.worktree.0, &["remote", "get-url", name])
            .await;
        if result.exit_code != 0 {
            return None;
        }
        let trimmed = result.text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    // history ---------------------------------------------------------------

    /// `history.head(repository)`.
    pub async fn history_head(&self, repository: &Repository) -> Option<String> {
        let result = self
            .run(&repository.worktree.0, &["rev-parse", "HEAD"])
            .await;
        if result.exit_code != 0 {
            return None;
        }
        let trimmed = result.text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// `history.branch(repository)`.
    pub async fn history_branch(&self, repository: &Repository) -> Option<String> {
        let result = self
            .run(
                &repository.worktree.0,
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
            )
            .await;
        if result.exit_code != 0 {
            return None;
        }
        let trimmed = result.text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// `history.defaultRemoteBranch(repository, remote)`.
    pub async fn history_default_remote_branch(
        &self,
        repository: &Repository,
        remote: &str,
    ) -> Option<String> {
        let refspec = format!("refs/remotes/{remote}/HEAD");
        let result = self
            .run(&repository.worktree.0, &["symbolic-ref", &refspec])
            .await;
        if result.exit_code != 0 {
            return None;
        }
        let prefix = format!("refs/remotes/{remote}/");
        let value = result
            .text
            .trim()
            .strip_prefix(&prefix)
            .unwrap_or_default()
            .to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    /// `history.rootCommits(repository)` — sorted root commits.
    pub async fn history_root_commits(&self, repository: &Repository) -> Vec<String> {
        let result = self
            .run(
                &repository.worktree.0,
                &["rev-list", "--max-parents=0", "HEAD"],
            )
            .await;
        if result.exit_code != 0 {
            return Vec::new();
        }
        let mut commits: Vec<String> = result
            .text
            .split('\n')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        commits.sort();
        commits
    }

    // sync ------------------------------------------------------------------

    /// `sync.fetchRemotes(repository, { prune })`.
    pub async fn sync_fetch_remotes(
        &self,
        repository: &Repository,
        prune: bool,
    ) -> Result<(), OperationError> {
        if prune {
            self.operation(
                "fetch",
                &repository.worktree,
                &["fetch", "--all", "--prune"],
            )
            .await
        } else {
            self.operation("fetch", &repository.worktree, &["fetch", "--all"])
                .await
        }
    }

    /// `sync.fetchBranch(repository, input)`.
    pub async fn sync_fetch_branch(
        &self,
        repository: &Repository,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> Result<(), OperationError> {
        let spec = format!("refs/heads/{branch}:refs/remotes/{remote}/{branch}");
        let spec = if force { format!("+{spec}") } else { spec };
        let args = ["fetch", remote, spec.as_str()];
        self.operation("fetch", &repository.worktree, &args).await
    }

    /// `sync.checkoutRemoteBranch(repository, input)`.
    pub async fn sync_checkout_remote_branch(
        &self,
        repository: &Repository,
        remote: &str,
        branch: &str,
        reset: bool,
    ) -> Result<(), OperationError> {
        let args: Vec<String> = if reset {
            vec![
                "checkout".into(),
                "-B".into(),
                branch.to_string(),
                format!("{remote}/{branch}"),
            ]
        } else {
            vec!["checkout".into(), branch.to_string()]
        };
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.operation("checkout", &repository.worktree, &refs)
            .await
    }

    /// `sync.resetHard(repository, revision)`.
    pub async fn sync_reset_hard(
        &self,
        repository: &Repository,
        revision: &str,
    ) -> Result<(), OperationError> {
        self.operation(
            "reset",
            &repository.worktree,
            &["reset", "--hard", revision],
        )
        .await
    }

    // change ----------------------------------------------------------------

    /// `change.capture(input)`.
    pub async fn change_capture(
        &self,
        repository: &Repository,
        path: &AbsolutePath,
    ) -> Result<ChangeSet, PatchError> {
        let scope = relative_scope(&repository.worktree.0, &path.0);
        let tracked = self
            .execute(
                &repository.worktree.0,
                &["diff", "--binary", "HEAD", "--", &scope],
            )
            .await
            .map_err(|e| patch_with_cause("capture", path, e.to_string()))?;
        if tracked.exit_code != 0 {
            return Err(PatchError::new(
                "capture",
                path,
                first_non_empty(
                    &tracked.stderr,
                    &tracked.text,
                    "Failed to capture tracked changes",
                ),
            ));
        }
        let untracked = self
            .execute(
                &repository.worktree.0,
                &[
                    "ls-files",
                    "--others",
                    "--exclude-standard",
                    "-z",
                    "--",
                    &scope,
                ],
            )
            .await
            .map_err(|e| patch_with_cause("capture", path, e.to_string()))?;
        if untracked.exit_code != 0 {
            return Err(PatchError::new(
                "capture",
                path,
                first_non_empty(
                    &untracked.stderr,
                    &untracked.text,
                    "Failed to list untracked changes",
                ),
            ));
        }
        let mut patches = vec![tracked.text];
        for file in untracked.text.split('\0').filter(|item| !item.is_empty()) {
            let result = self
                .execute(
                    &repository.worktree.0,
                    &["diff", "--binary", "--no-index", "--", "/dev/null", file],
                )
                .await
                .map_err(|e| patch_with_cause("capture", path, e.to_string()))?;
            // git diff --no-index returns 1 when differences were found.
            if result.exit_code == 0 || result.exit_code == 1 {
                patches.push(result.text);
            } else {
                return Err(PatchError::new(
                    "capture",
                    path,
                    first_non_empty(
                        &result.stderr,
                        &result.text,
                        &format!("Failed to capture untracked change: {file}"),
                    ),
                ));
            }
        }
        Ok(ChangeSet(
            patches
                .iter()
                .filter(|p| !p.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }

    /// `change.apply(input)`.
    pub async fn change_apply(
        &self,
        path: &AbsolutePath,
        changes: &ChangeSet,
    ) -> Result<(), PatchError> {
        let result = self
            .execute_with_stdin(
                &path.0,
                &["apply", "-"],
                Some(changes.0.as_bytes().to_vec()),
            )
            .await
            .map_err(|e| patch_with_cause("apply", path, e.to_string()))?;
        if result.exit_code == 0 {
            return Ok(());
        }
        Err(PatchError::new(
            "apply",
            path,
            first_non_empty(&result.stderr, &result.text, "Failed to apply changes"),
        ))
    }

    /// `change.discard(input)`.
    pub async fn change_discard(
        &self,
        repository: &Repository,
        path: &AbsolutePath,
        index: IndexMode,
        untracked: UntrackedMode,
    ) -> Result<(), PatchError> {
        let scope = relative_scope(&repository.worktree.0, &path.0);
        let restore_args: Vec<String> = if index == IndexMode::Reset {
            vec!["checkout".into(), "HEAD".into(), "--".into(), scope.clone()]
        } else {
            vec!["checkout".into(), "--".into(), scope.clone()]
        };
        let restore_refs: Vec<&str> = restore_args.iter().map(|s| s.as_str()).collect();
        let restore = self
            .execute(&repository.worktree.0, &restore_refs)
            .await
            .map_err(|e| patch_with_cause("reset", path, e.to_string()))?;
        if restore.exit_code != 0 {
            return Err(PatchError::new(
                "reset",
                path,
                first_non_empty(
                    &restore.stderr,
                    &restore.text,
                    "Failed to restore tracked changes",
                ),
            ));
        }
        if untracked == UntrackedMode::Preserve {
            return Ok(());
        }
        let clean = self
            .execute(&repository.worktree.0, &["clean", "-fd", "--", &scope])
            .await
            .map_err(|e| patch_with_cause("reset", path, e.to_string()))?;
        if clean.exit_code == 0 {
            return Ok(());
        }
        Err(PatchError::new(
            "reset",
            path,
            first_non_empty(
                &clean.stderr,
                &clean.text,
                "Failed to clean untracked changes",
            ),
        ))
    }

    // worktree --------------------------------------------------------------

    /// `worktree.create(input)`.
    pub async fn worktree_create(
        &self,
        repository: &Repository,
        directory: &AbsolutePath,
    ) -> Result<Repository, WorktreeError> {
        let args = ["worktree", "add", "--detach", directory.0.as_str(), "HEAD"];
        self.worktree_run(
            "create",
            repository,
            &args,
            Some(directory),
            &repository.worktree.0,
        )
        .await?;
        match self.discover(&directory.0).await {
            Some(repository) => Ok(repository),
            None => Err(WorktreeError {
                _tag: "Git.WorktreeError".to_string(),
                operation: "create".to_string(),
                message: format!("Created worktree could not be opened: {}", directory.0),
                directory: Some(directory.clone()),
                force_required: None,
                cause: None,
            }),
        }
    }

    /// `worktree.remove(input)`.
    pub async fn worktree_remove(
        &self,
        repository: &Repository,
        directory: &AbsolutePath,
        force: bool,
    ) -> Result<(), WorktreeError> {
        let args: Vec<String> = if force {
            vec![
                "worktree".into(),
                "remove".into(),
                "--force".into(),
                directory.0.clone(),
            ]
        } else {
            vec!["worktree".into(), "remove".into(), directory.0.clone()]
        };
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.worktree_run(
            "remove",
            repository,
            &refs,
            Some(directory),
            &repository.commonDirectory.0,
        )
        .await?;
        Ok(())
    }

    /// `worktree.list(repository)`.
    pub async fn worktree_list(
        &self,
        repository: &Repository,
    ) -> Result<Vec<GitWorktree>, WorktreeError> {
        let text = self
            .worktree_run(
                "list",
                repository,
                &["worktree", "list", "--porcelain"],
                None,
                &repository.worktree.0,
            )
            .await?;
        let mut result = Vec::new();
        for (index, line) in text.split('\n').enumerate() {
            if let Some(value) = line.strip_prefix("worktree ") {
                result.push(GitWorktree {
                    directory: AbsolutePath(resolve_path(&repository.worktree.0, value.trim())),
                    kind: if index == 0 {
                        "main".to_string()
                    } else {
                        "linked".to_string()
                    },
                });
            }
        }
        Ok(result)
    }

    // index -----------------------------------------------------------------

    /// `index.refresh(input)`.
    pub async fn index_refresh(
        &self,
        repository: &Repository,
        scope: &RelativePath,
        ignores: Option<&Repository>,
        maximum_untracked_file_bytes: Option<u64>,
    ) -> Result<IndexRefresh, OperationError> {
        let tracked = self
            .repository_operation(
                "refresh",
                repository,
                &["diff-files", "--name-only", "-z", "--", &scope.0],
            )
            .await?
            .0
            .split('\0')
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
            .collect::<Vec<String>>();
        let untracked = self
            .repository_operation(
                "refresh",
                repository,
                &[
                    "ls-files",
                    "--others",
                    "--exclude-standard",
                    "-z",
                    "--",
                    &scope.0,
                ],
            )
            .await?
            .0
            .split('\0')
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
            .collect::<Vec<String>>();

        let mut candidates: Vec<String> = tracked;
        for item in &untracked {
            if !candidates.contains(item) {
                candidates.push(item.clone());
            }
        }
        if candidates.is_empty() {
            return Ok(IndexRefresh {
                skipped: Vec::new(),
            });
        }

        let ignored: std::collections::HashSet<String> = if let Some(ignores) = ignores {
            let stdin = format!("{}\0", candidates.join("\0"));
            let result = self
                .repository_operation_opts(
                    "refresh",
                    ignores,
                    &["check-ignore", "--no-index", "--stdin", "-z"],
                    OperationOptions {
                        stdin: Some(stdin.into_bytes()),
                        env: None,
                    },
                )
                .await
                .unwrap_or_else(|_| (String::new(), String::new()));
            result
                .0
                .split('\0')
                .filter(|item| !item.is_empty())
                .map(|item| item.to_string())
                .collect()
        } else {
            std::collections::HashSet::new()
        };

        let allowed: Vec<String> = candidates
            .iter()
            .filter(|item| !ignored.contains(*item))
            .cloned()
            .collect();
        let skipped: Vec<RelativePath> = if let Some(maximum) = maximum_untracked_file_bytes {
            let mut skipped = Vec::new();
            for item in untracked.iter().filter(|item| allowed.contains(item)) {
                let path = Path::new(&repository.worktree.0).join(item);
                if let Ok(Some(info)) = self.fs.stat(&path.display().to_string()).await {
                    if info.kind == crate::fs_util::Kind::File && info.size > maximum {
                        skipped.push(RelativePath(item.clone()));
                    }
                }
            }
            skipped
        } else {
            Vec::new()
        };

        let stage: Vec<String> = allowed
            .iter()
            .filter(|item| !skipped.contains(&RelativePath((*item).clone())))
            .cloned()
            .collect();
        let mut remove: Vec<String> = ignored.into_iter().collect();
        remove.extend(skipped.iter().map(|item| item.0.clone()));

        if !remove.is_empty() {
            self.repository_operation_opts(
                "refresh",
                repository,
                &[
                    "rm",
                    "--cached",
                    "-f",
                    "--ignore-unmatch",
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ],
                OperationOptions {
                    stdin: Some(format!("{}\0", remove.join("\0")).into_bytes()),
                    env: None,
                },
            )
            .await?;
        }
        if !stage.is_empty() {
            self.repository_operation_opts(
                "refresh",
                repository,
                &[
                    "add",
                    "--all",
                    "--sparse",
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ],
                OperationOptions {
                    stdin: Some(format!("{}\0", stage.join("\0")).into_bytes()),
                    env: None,
                },
            )
            .await?;
        }
        Ok(IndexRefresh { skipped })
    }

    /// `index.ignored(input)`.
    pub async fn index_ignored(
        &self,
        repository: &Repository,
        paths: &[RelativePath],
    ) -> Result<Vec<RelativePath>, OperationError> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let stdin = format!(
            "{}\0",
            paths
                .iter()
                .map(|p| p.0.clone())
                .collect::<Vec<_>>()
                .join("\0")
        );
        let mut command =
            self.git_command(repository, &["check-ignore", "--no-index", "--stdin", "-z"]);
        command.stdin = Stdin::Pipe;
        let options = RunOptions {
            stdin: Some(stdin.into_bytes()),
            ..RunOptions::default()
        };
        let result = process::run(&command, &options)
            .await
            .map_err(|cause| OperationError {
                _tag: "Git.OperationError".to_string(),
                operation: "list_files".to_string(),
                message: cause.to_string(),
                directory: Some(repository.worktree.clone()),
                cause: Some(cause.to_string()),
            })?;
        if result.exit_code != 0 && result.exit_code != 1 {
            let message = String::from_utf8_lossy(&result.stderr).trim().to_string();
            return Err(OperationError {
                _tag: "Git.OperationError".to_string(),
                operation: "list_files".to_string(),
                message: if message.is_empty() {
                    "Failed to check ignored paths".to_string()
                } else {
                    message
                },
                directory: Some(repository.worktree.clone()),
                cause: None,
            });
        }
        Ok(String::from_utf8_lossy(&result.stdout)
            .split('\0')
            .filter(|item| !item.is_empty())
            .map(|item| RelativePath(item.to_string()))
            .collect())
    }

    // tree ------------------------------------------------------------------

    /// `tree.write(repository)`.
    pub async fn tree_write(&self, repository: &Repository) -> Result<TreeId, OperationError> {
        let (text, _) = self
            .repository_operation("write_tree", repository, &["write-tree"])
            .await?;
        Ok(TreeId(text.trim().to_string()))
    }

    /// `tree.capture(input)`.
    pub async fn tree_capture(
        &self,
        repository: &Repository,
        scopes: &[RelativePath],
        ignores: Option<&Repository>,
        maximum_untracked_file_bytes: Option<u64>,
    ) -> Result<TreeId, OperationError> {
        self.locked(repository, async {
            for scope in scopes {
                self.index_refresh(repository, scope, ignores, maximum_untracked_file_bytes)
                    .await?;
            }
            self.tree_write(repository).await
        })
        .await
    }

    /// `tree.files(input)`.
    pub async fn tree_files(
        &self,
        repository: &Repository,
        from: &TreeId,
        to: &TreeId,
    ) -> Result<Vec<RelativePath>, OperationError> {
        let (text, _) = self
            .repository_operation(
                "list_files",
                repository,
                &["diff", "--name-only", "-z", &from.0, &to.0],
            )
            .await?;
        Ok(text
            .split('\0')
            .filter(|item| !item.is_empty())
            .map(|item| RelativePath(item.to_string()))
            .collect())
    }

    /// `tree.diff(input)`.
    pub async fn tree_diff(
        &self,
        repository: &Repository,
        from: &TreeId,
        to: &TreeId,
        context: Option<u32>,
        paths: Option<Vec<RelativePath>>,
    ) -> Result<Vec<FileDiff>, OperationError> {
        let paths = match paths {
            Some(paths) => paths,
            None => self.tree_files(repository, from, to).await?,
        };
        let mut diffs = Vec::with_capacity(paths.len());
        for file in paths {
            let (status_text, _) = self
                .repository_operation(
                    "diff",
                    repository,
                    &[
                        "diff",
                        "--name-status",
                        "--no-renames",
                        &from.0,
                        &to.0,
                        "--",
                        &file.0,
                    ],
                )
                .await?;
            let status_text = status_text.trim().to_string();
            let status = if status_text.starts_with('A') {
                "added".to_string()
            } else if status_text.starts_with('D') {
                "deleted".to_string()
            } else {
                "modified".to_string()
            };
            let (stats_text, _) = self
                .repository_operation(
                    "diff",
                    repository,
                    &[
                        "diff",
                        "--numstat",
                        "--no-renames",
                        &from.0,
                        &to.0,
                        "--",
                        &file.0,
                    ],
                )
                .await?;
            let stats: Vec<&str> = stats_text.split('\t').collect();
            let binary = stats.first().copied().unwrap_or_default() == "-"
                || stats.get(1).copied().unwrap_or_default() == "-";
            let patch = if binary {
                String::new()
            } else {
                let unified = format!("--unified={}", context.unwrap_or(3));
                self.repository_operation(
                    "diff",
                    repository,
                    &[
                        "diff",
                        &unified,
                        "--no-renames",
                        &from.0,
                        &to.0,
                        "--",
                        &file.0,
                    ],
                )
                .await?
                .0
            };
            let parse_num = |value: Option<&&str>| -> u64 {
                value.copied().and_then(|v| v.parse().ok()).unwrap_or(0)
            };
            diffs.push(FileDiff {
                path: file,
                status,
                additions: if binary { 0 } else { parse_num(stats.first()) },
                deletions: if binary { 0 } else { parse_num(stats.get(1)) },
                patch,
            });
        }
        Ok(diffs)
    }

    /// `tree.preview(input)`.
    pub async fn tree_preview(
        &self,
        repository: &Repository,
        current: &TreeId,
        files: &BTreeMap<RelativePath, TreeId>,
        context: Option<u32>,
    ) -> Result<Vec<FileDiff>, OperationError> {
        self.locked(repository, async {
            let index = format!("{}/preview-{}.index", repository.gitDirectory.0, uuid4());
            let env = BTreeMap::from_iter([("GIT_INDEX_FILE".to_string(), index.clone())]);
            let result = async {
                self.repository_operation_opts(
                    "diff",
                    repository,
                    &["read-tree", &current.0],
                    OperationOptions {
                        stdin: None,
                        env: Some(env.clone()),
                    },
                )
                .await?;
                for (file, tree) in files {
                    let source = self.tree_entry(repository, tree, file).await?;
                    match source {
                        Some((mode, object)) => {
                            self.repository_operation_opts(
                                "diff",
                                repository,
                                &[
                                    "update-index",
                                    "--add",
                                    "--cacheinfo",
                                    &mode,
                                    &object,
                                    &file.0,
                                ],
                                OperationOptions {
                                    stdin: None,
                                    env: Some(env.clone()),
                                },
                            )
                            .await?;
                        }
                        None => {
                            self.repository_operation_opts(
                                "diff",
                                repository,
                                &["update-index", "--force-remove", "--", &file.0],
                                OperationOptions {
                                    stdin: None,
                                    env: Some(env.clone()),
                                },
                            )
                            .await?;
                        }
                    }
                }
                let (target, _) = self
                    .repository_operation_opts(
                        "diff",
                        repository,
                        &["write-tree"],
                        OperationOptions {
                            stdin: None,
                            env: Some(env.clone()),
                        },
                    )
                    .await?;
                let target = TreeId(target.trim().to_string());
                let paths = files.keys().cloned().collect();
                self.tree_diff(repository, current, &target, context, Some(paths))
                    .await
            }
            .await;
            let _ = self.fs.remove(&index, false, true).await;
            result
        })
        .await
    }

    /// `tree.restore(input)`.
    pub async fn tree_restore(
        &self,
        repository: &Repository,
        files: &BTreeMap<RelativePath, TreeId>,
    ) -> Result<(), OperationError> {
        self.locked(repository, async {
            for (file, tree) in files {
                if self.tree_entry(repository, tree, file).await?.is_some() {
                    self.repository_operation(
                        "restore",
                        repository,
                        &["checkout", &tree.0, "--", &file.0],
                    )
                    .await?;
                } else {
                    let path = Path::new(&repository.worktree.0).join(&file.0);
                    self.fs
                        .remove(&path.display().to_string(), true, true)
                        .await
                        .map_err(|_| {
                            OperationError::new("restore", format!("Failed to remove {file}"))
                        })?;
                }
            }
            Ok(())
        })
        .await
    }

    /// `tree.checkout(input)`.
    pub async fn tree_checkout(
        &self,
        repository: &Repository,
        tree: &TreeId,
    ) -> Result<(), OperationError> {
        self.locked(repository, async {
            self.repository_operation("restore", repository, &["read-tree", &tree.0])
                .await?;
            self.repository_operation(
                "restore",
                repository,
                &["checkout-index", "--all", "--force"],
            )
            .await?;
            Ok(())
        })
        .await
    }

    // internals -------------------------------------------------------------

    async fn tree_entry(
        &self,
        repository: &Repository,
        tree: &TreeId,
        file: &RelativePath,
    ) -> Result<Option<(String, String)>, OperationError> {
        let (text, _) = self
            .repository_operation(
                "restore",
                repository,
                &["ls-tree", "-z", &tree.0, "--", &file.0],
            )
            .await?;
        let text = text.trim_end_matches('\0');
        if text.is_empty() {
            return Ok(None);
        }
        // /^(\d+)\s+\w+\s+([0-9a-f]+)\t/
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == 0 {
            return Err(OperationError::new(
                "restore",
                format!("Invalid tree entry for {file}"),
            ));
        }
        let mode = text[..i].to_string();
        // skip whitespace + type (alnum) + whitespace
        let rest = &text[i..];
        let rest = rest.trim_start();
        let mut j = 0;
        while j < rest.len() && (rest.as_bytes()[j].is_ascii_alphanumeric()) {
            j += 1;
        }
        let rest = rest[j..].trim_start();
        let object: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if object.is_empty() {
            return Err(OperationError::new(
                "restore",
                format!("Invalid tree entry for {file}"),
            ));
        }
        Ok(Some((mode, object)))
    }

    /// Run a stateful tree operation under the per-repository lock.
    async fn locked<T, F>(&self, repository: &Repository, effect: F) -> Result<T, OperationError>
    where
        F: std::future::Future<Output = Result<T, OperationError>>,
    {
        self.locks
            .with_lock(repository.gitDirectory.0.clone(), effect)
            .await
    }

    /// `operation(...)` — a plain (cwd-scoped) git invocation mapped to
    /// `OperationError` on failure.
    async fn operation(
        &self,
        operation: &str,
        directory: &AbsolutePath,
        args: &[&str],
    ) -> Result<(), OperationError> {
        let result = self
            .execute(&directory.0, args)
            .await
            .map_err(|cause| OperationError {
                _tag: "Git.OperationError".to_string(),
                operation: operation.to_string(),
                message: cause.to_string(),
                directory: Some(directory.clone()),
                cause: Some(cause.to_string()),
            })?;
        if result.exit_code == 0 {
            return Ok(());
        }
        Err(OperationError {
            _tag: "Git.OperationError".to_string(),
            operation: operation.to_string(),
            message: fallback_message(&result, &format!("Git {operation} failed")),
            directory: Some(directory.clone()),
            cause: None,
        })
    }

    /// `repositoryOperation(...)` — git with `--git-dir`/`--work-tree`.
    async fn repository_operation(
        &self,
        operation: &str,
        repository: &Repository,
        args: &[&str],
    ) -> Result<(String, String), OperationError> {
        self.repository_operation_opts(operation, repository, args, OperationOptions::default())
            .await
    }

    async fn repository_operation_opts(
        &self,
        operation: &str,
        repository: &Repository,
        args: &[&str],
        options: OperationOptions,
    ) -> Result<(String, String), OperationError> {
        let mut command = self.git_command(repository, args);
        let mut run_options = RunOptions::default();
        if let Some(env) = options.env {
            command.env = Some(env);
        }
        if let Some(stdin) = options.stdin {
            command.stdin = Stdin::Pipe;
            run_options.stdin = Some(stdin);
        }
        let result = process::run(&command, &run_options)
            .await
            .map_err(|cause| OperationError {
                _tag: "Git.OperationError".to_string(),
                operation: operation.to_string(),
                message: cause.to_string(),
                directory: Some(repository.worktree.clone()),
                cause: Some(cause.to_string()),
            })?;
        let text = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();
        if result.exit_code == 0 {
            return Ok((text, stderr));
        }
        let message = fallback_message_raw(&stderr, &text, &format!("Git {operation} failed"));
        Err(OperationError {
            _tag: "Git.OperationError".to_string(),
            operation: operation.to_string(),
            message,
            directory: Some(repository.worktree.clone()),
            cause: None,
        })
    }

    fn git_command(&self, repository: &Repository, args: &[&str]) -> Command {
        let mut full = vec![
            "--git-dir".to_string(),
            repository.gitDirectory.0.clone(),
            "--work-tree".to_string(),
            repository.worktree.0.clone(),
        ];
        full.extend(args.iter().map(|arg| arg.to_string()));
        let mut command = Command::new("git", full);
        command.cwd = Some(repository.worktree.0.clone());
        command.extend_env = true;
        command
    }

    /// `execute(cwd, proc)(args)` — propagates spawn errors.
    async fn execute(
        &self,
        cwd: &str,
        args: &[&str],
    ) -> Result<ExecResult, process::AppProcessError> {
        self.execute_with_stdin(cwd, args, None).await
    }

    async fn execute_with_stdin(
        &self,
        cwd: &str,
        args: &[&str],
        stdin: Option<Vec<u8>>,
    ) -> Result<ExecResult, process::AppProcessError> {
        let mut command = Command::new("git", args.iter().map(|s| s.to_string()).collect());
        command.cwd = Some(cwd.to_string());
        command.extend_env = true;
        if stdin.is_some() {
            command.stdin = Stdin::Pipe;
        }
        let result = process::run(
            &command,
            &RunOptions {
                stdin,
                ..RunOptions::default()
            },
        )
        .await?;
        Ok(ExecResult {
            exit_code: result.exit_code,
            text: String::from_utf8_lossy(&result.stdout).to_string(),
            stderr: String::from_utf8_lossy(&result.stderr).to_string(),
        })
    }

    /// `run(cwd, proc)(args)` — swallows spawn errors like the reference.
    async fn run(&self, cwd: &str, args: &[&str]) -> ExecResult {
        match self.execute(cwd, args).await {
            Ok(result) => result,
            Err(_) => ExecResult {
                exit_code: 1,
                text: String::new(),
                stderr: String::new(),
            },
        }
    }

    async fn worktree_run(
        &self,
        operation: &str,
        _repository: &Repository,
        args: &[&str],
        worktree_directory: Option<&AbsolutePath>,
        cwd: &str,
    ) -> Result<String, WorktreeError> {
        let mut command = Command::new("git", args.iter().map(|s| s.to_string()).collect());
        command.cwd = Some(cwd.to_string());
        command.extend_env = true;
        command.stdin = Stdin::Ignore;
        let result = process::run(&command, &RunOptions::default())
            .await
            .map_err(|cause| WorktreeError {
                _tag: "Git.WorktreeError".to_string(),
                operation: operation.to_string(),
                message: cause.to_string(),
                directory: worktree_directory.cloned(),
                force_required: None,
                cause: Some(cause.to_string()),
            })?;
        if result.exit_code == 0 {
            return Ok(String::from_utf8_lossy(&result.stdout).to_string());
        }
        let stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            String::from_utf8_lossy(&result.stdout).trim().to_string()
        } else {
            stderr
        };
        let message = if message.is_empty() {
            "Git failed".to_string()
        } else {
            message
        };
        let force_required = operation == "remove" && regex_dirty(&message);
        Err(WorktreeError {
            _tag: "Git.WorktreeError".to_string(),
            operation: operation.to_string(),
            message,
            directory: worktree_directory.cloned(),
            force_required: Some(force_required),
            cause: None,
        })
    }
}

/// `index.refresh` result — `{ skipped }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRefresh {
    pub skipped: Vec<RelativePath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Preserve,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntrackedMode {
    Preserve,
    Remove,
}

fn fallback_message(result: &ExecResult, default: &str) -> String {
    fallback_message_raw(&result.stderr, &result.text, default)
}

fn fallback_message_raw(stderr: &str, stdout: &str, default: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }
    let stdout = stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }
    default.to_string()
}

fn patch_with_cause(operation: &str, directory: &AbsolutePath, cause: String) -> PatchError {
    PatchError {
        _tag: "Git.PatchError".to_string(),
        operation: operation.to_string(),
        directory: directory.clone(),
        message: cause.clone(),
        cause: Some(cause),
    }
}

fn relative_scope(worktree: &str, path: &str) -> String {
    // `git rev-parse --show-toplevel` returns the canonical worktree on macOS
    // (`/private/var/...`) while callers may retain the `/var/...` spelling.
    // Normalize both sides before deriving the pathspec.
    let canonical_worktree =
        std::fs::canonicalize(worktree).unwrap_or_else(|_| Path::new(worktree).to_path_buf());
    let canonical_path =
        std::fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf());
    let relative = canonical_path
        .strip_prefix(&canonical_worktree)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
        .trim_start_matches('/')
        .to_string();
    let scope = relative.replace('\\', "/");
    if scope.is_empty() {
        ".".to_string()
    } else {
        scope
    }
}

fn resolve_path(cwd: &str, value: &str) -> String {
    let trimmed = trim_newlines(value);
    if trimmed.is_empty() {
        return cwd.to_string();
    }
    let normalized = crate::fs_util::windows_path(trimmed);
    if Path::new(&normalized).is_absolute() {
        Path::new(&normalized).display().to_string()
    } else {
        Path::new(cwd).join(normalized).display().to_string()
    }
}

fn regex_dirty(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("contains modified or untracked files") || lower.contains("is dirty")
}

fn uuid4() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("RNG");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut hex = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if index == 4 || index == 6 || index == 8 || index == 10 {
            hex.push('-');
        }
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn first_non_empty(stderr: &str, stdout: &str, default: &str) -> String {
    fallback_message_raw(stderr, stdout, default)
}
