/// From reference/packages/opencode/src/project/vcs.ts
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use regex::Regex;

use crate::git::{Git, Item, Kind, Patch, PatchOptions};
use crate::schema::{
    PatchApplyError, PatchApplyReason, VcsApplyInput, VcsApplyResult, VcsFileDiff, VcsFileStatus,
};
use crate::util::bus::{Bus, EventPayload};
use crate::util::diff::{format_patch, structured_patch};
use crate::util::GitResult;

const PATCH_CONTEXT_LINES: usize = 2_147_483_647;
const MAX_PATCH_BYTES: usize = 10_000_000;
const MAX_TOTAL_PATCH_BYTES: usize = 10_000_000;

/// `Vcs.Mode`: `"git" | "branch"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Git,
    Branch,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DiffOptions {
    pub context: Option<usize>,
}

fn empty_patch(file: &str) -> String {
    format_patch(&structured_patch(file, file, "", "", 0))
}

fn nums(list: &[crate::git::Stat]) -> HashMap<String, (u64, u64)> {
    let mut map = HashMap::new();
    for item in list {
        map.insert(item.file.clone(), (item.additions, item.deletions));
    }
    map
}

fn merge(lists: &[&[Item]]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in lists.iter().flat_map(|list| list.iter()) {
        if seen.insert(item.file.clone()) {
            out.push(item.clone());
        }
    }
    out
}

fn empty_batch() -> (HashMap<String, String>, bool) {
    (HashMap::new(), false)
}

/// Parses a git-quoted path token, unescaping `\t` `\n` `\r` and `\"`.
fn parse_quoted_path(value: &str) -> Option<(String, usize)> {
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut idx = 1;
    while idx < bytes.len() {
        let char = bytes[idx] as char;
        if char == '"' {
            return Some((out, idx + 1));
        }
        if char != '\\' {
            out.push(char);
            idx += 1;
            continue;
        }
        idx += 1;
        let next = bytes.get(idx).map(|b| *b as char);
        match next {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('"') | Some('\\') => out.push(next.unwrap()),
            Some(other) => out.push(other),
            None => {}
        }
        idx += 1;
    }
    None
}

fn parse_path_token(value: &str) -> String {
    if !value.starts_with('"') {
        return value.split('\t').next().unwrap_or(value).to_string();
    }
    parse_quoted_path(value)
        .map(|(value, _)| value)
        .unwrap_or_else(|| value.to_string())
}

fn file_from_diff_path(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.is_empty() || value == "/dev/null" {
        return None;
    }
    let file = parse_path_token(value);
    if let Some(rest) = file.strip_prefix("a/") {
        Some(rest.to_string())
    } else if let Some(rest) = file.strip_prefix("b/") {
        Some(rest.to_string())
    } else {
        Some(file)
    }
}

fn file_from_git_header(header: &str) -> Option<String> {
    if header.starts_with('"') {
        let first = parse_quoted_path(header);
        let second = first
            .and_then(|(_, end)| header.get(end..))
            .map(|s| s.trim_start());
        let second = second?;
        if second.is_empty() {
            return None;
        }
        if !second.starts_with('"') {
            return file_from_diff_path(Some(second));
        }
        return file_from_diff_path(parse_quoted_path(second).map(|(value, _)| value).as_deref());
    }

    let separator = header.find(" b/")?;
    file_from_diff_path(header.get(separator + 1..))
}

fn file_from_patch_chunk(chunk: &str) -> Option<String> {
    let next_re = Regex::new(r"(?m)^\+\+\+ (.+)$").unwrap();
    let before_re = Regex::new(r"(?m)^--- (.+)$").unwrap();
    let next = next_re
        .captures(chunk)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string());
    let before = before_re
        .captures(chunk)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string());
    let file =
        file_from_diff_path(next.as_deref()).or_else(|| file_from_diff_path(before.as_deref()));
    if file.is_some() {
        return file;
    }

    let header_re = Regex::new(r"(?m)^diff --git (.+)$").unwrap();
    let header = header_re
        .captures(chunk)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string());
    file_from_git_header(header.as_deref().unwrap_or(""))
}

fn split_git_patch(patch: &Patch) -> Vec<String> {
    let re = Regex::new(r"(?:^|\n)diff --git ").unwrap();
    let mut starts: Vec<usize> = Vec::new();
    for matched in re.find_iter(&patch.text) {
        let start = if patch.text.as_bytes().get(matched.start()) == Some(&b'\n') {
            matched.start() + 1
        } else {
            matched.start()
        };
        starts.push(start);
    }
    let mut chunks = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(patch.text.len());
        chunks.push(patch.text[*start..end].to_string());
    }
    if !patch.truncated {
        return chunks;
    }
    chunks.truncate(chunks.len().saturating_sub(1));
    chunks
}

async fn batch_patches(
    git: &Git,
    cwd: &str,
    r#ref: &str,
    list: &[Item],
    options: DiffOptions,
) -> (HashMap<String, String>, bool) {
    if list.is_empty() {
        return empty_batch();
    }
    let result = git
        .patch_all(
            cwd,
            r#ref,
            Some(PatchOptions {
                context: Some(options.context.unwrap_or(PATCH_CONTEXT_LINES)),
                max_output_bytes: Some(MAX_TOTAL_PATCH_BYTES),
            }),
        )
        .await;
    let chunks = split_git_patch(&result);
    let mut patches: HashMap<String, String> = HashMap::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let file =
            file_from_patch_chunk(chunk).or_else(|| list.get(index).map(|item| item.file.clone()));
        if let Some(file) = file {
            patches.entry(file).or_default().push_str(chunk);
        }
    }
    (patches, result.truncated)
}

async fn native_patch(
    git: &Git,
    cwd: &str,
    r#ref: Option<&str>,
    item: &Item,
    options: DiffOptions,
) -> String {
    let options = PatchOptions {
        context: Some(options.context.unwrap_or(PATCH_CONTEXT_LINES)),
        max_output_bytes: Some(MAX_PATCH_BYTES),
    };
    let result = if item.code == "??" || r#ref.is_none() {
        git.patch_untracked(cwd, &item.file, Some(options)).await
    } else {
        git.patch(cwd, r#ref.unwrap(), &item.file, Some(options))
            .await
    };
    if !result.truncated && !result.text.is_empty() {
        return result.text;
    }
    empty_patch(&item.file)
}

fn total_patch(file: &str, patch: &str, total: usize) -> (String, bool) {
    if total + patch.as_bytes().len() <= MAX_TOTAL_PATCH_BYTES {
        return (patch.to_string(), false);
    }
    (empty_patch(file), true)
}

async fn patch_for_item(
    git: &Git,
    cwd: &str,
    r#ref: Option<&str>,
    item: &Item,
    batch: &(HashMap<String, String>, bool),
    capped: bool,
    options: DiffOptions,
) -> String {
    if capped {
        return empty_patch(&item.file);
    }
    if let Some(batched) = batch.0.get(&item.file) {
        return batched.clone();
    }
    if item.code != "??" && batch.1 {
        return empty_patch(&item.file);
    }
    native_patch(git, cwd, r#ref, item, options).await
}

async fn files(
    git: &Git,
    cwd: &str,
    r#ref: Option<&str>,
    list: Vec<Item>,
    map: HashMap<String, (u64, u64)>,
    batch: (HashMap<String, String>, bool),
    options: DiffOptions,
) -> Vec<VcsFileDiff> {
    let mut next = Vec::new();
    let mut total = 0usize;
    let mut capped = false;

    let mut sorted = list;
    sorted.sort_by(|a, b| a.file.cmp(&b.file));

    for item in sorted {
        let stat = if let Some(stat) = map.get(&item.file) {
            Some(*stat)
        } else if item.status == Kind::Added {
            git.stat_untracked(cwd, &item.file)
                .await
                .map(|stat| (stat.additions, stat.deletions))
        } else {
            None
        };
        let patch = patch_for_item(git, cwd, r#ref, &item, &batch, capped, options).await;
        let result = if capped {
            (patch, true)
        } else {
            total_patch(&item.file, &patch, total)
        };
        capped = capped || result.1;
        if !capped {
            total += result.0.as_bytes().len();
            capped = total >= MAX_TOTAL_PATCH_BYTES;
        }
        next.push(VcsFileDiff {
            file: item.file,
            patch: Some(result.0),
            additions: stat.map(|s| s.0).unwrap_or(0),
            deletions: stat.map(|s| s.1).unwrap_or(0),
            status: Some(item.status.as_str().to_string()),
        });
    }

    next
}

async fn diff_against_ref(
    git: &Git,
    cwd: &str,
    r#ref: &str,
    options: DiffOptions,
) -> Vec<VcsFileDiff> {
    let (list, stats, extra) = (
        git.diff(cwd, r#ref).await,
        git.stats(cwd, r#ref).await,
        git.status(cwd).await,
    );
    let untracked: Vec<Item> = extra.into_iter().filter(|item| item.code == "??").collect();
    files(
        git,
        cwd,
        Some(r#ref),
        merge(&[&list, &untracked]),
        nums(&stats),
        batch_patches(git, cwd, r#ref, &list, options).await,
        options,
    )
    .await
}

async fn track(
    git: &Git,
    cwd: &str,
    r#ref: Option<&str>,
    options: DiffOptions,
) -> Vec<VcsFileDiff> {
    match r#ref {
        None => {
            files(
                git,
                cwd,
                None,
                git.status(cwd).await,
                HashMap::new(),
                empty_batch(),
                options,
            )
            .await
        }
        Some(r#ref) => diff_against_ref(git, cwd, r#ref, options).await,
    }
}

#[derive(Debug, Default)]
struct State {
    current: Option<String>,
    root: Option<crate::git::Base>,
}

pub struct Vcs {
    pub git: Arc<Git>,
    pub bus: Arc<Bus>,
    states: Arc<Mutex<HashMap<String, Arc<Mutex<State>>>>>,
}

impl Clone for Vcs {
    fn clone(&self) -> Self {
        Vcs {
            git: self.git.clone(),
            bus: self.bus.clone(),
            states: self.states.clone(),
        }
    }
}

impl Vcs {
    pub fn new(git: Arc<Git>, bus: Arc<Bus>) -> Arc<Vcs> {
        Arc::new(Vcs {
            git,
            bus,
            states: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn state_for(&self, _ctx: &crate::schema::ProjectInfo, directory: &str) -> Arc<Mutex<State>> {
        if let Some(state) = self.states.lock().unwrap().get(directory) {
            return state.clone();
        }
        let state = Arc::new(Mutex::new(State {
            current: None,
            root: None,
        }));
        self.states
            .lock()
            .unwrap()
            .insert(directory.to_string(), state.clone());
        state
    }

    /// Loads the per-instance branch state and watches `HEAD` for changes,
    /// publishing `vcs.branch.updated` when the branch switches. The task exits
    /// when the instance's shutdown channel closes.
    pub async fn init(
        &self,
        ctx: crate::project::instance_context::InstanceContext,
        shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        let state = self.state_for(&ctx.project, &ctx.directory);
        if ctx.project.vcs.as_deref() != Some("git") {
            return tokio::spawn(async {});
        }
        let git = self.git.clone();
        let bus = self.bus.clone();
        let state = state.clone();
        let current = git.branch(&ctx.directory).await;
        let root = git.default_branch(&ctx.directory).await;
        {
            let mut guard = state.lock().unwrap();
            guard.current = current;
            guard.root = root;
        }
        let directory = ctx.directory.clone();
        let project_id = ctx.project.id.0.clone();
        tokio::spawn(async move {
            let mut listener = bus.listener();
            let mut shutdown = shutdown;
            loop {
                let event = tokio::select! {
                    event = listener.next(|r#type, location| {
                        r#type == crate::schema::WATCHER_UPDATED && location == Some(directory.as_str())
                    }) => event,
                    _ = shutdown.recv() => break,
                };
                let Some(event) = event else { break };
                let Some(data) = event.payload.data.as_ref() else {
                    continue;
                };
                let Some(file) = data.get("file").and_then(|value| value.as_str()) else {
                    continue;
                };
                if !file.ends_with("HEAD") {
                    continue;
                }
                let next = git.branch(&directory).await;
                let changed = {
                    let guard = state.lock().unwrap();
                    guard.current != next
                };
                if changed {
                    let properties = serde_json::to_value(crate::schema::VcsBranchUpdated {
                        branch: next.clone(),
                    })
                    .unwrap_or(serde_json::Value::Null);
                    bus.emit(crate::util::bus::BusEvent {
                        directory: directory.clone(),
                        project: Some(project_id.clone()),
                        workspace: None,
                        payload: EventPayload {
                            r#type: crate::schema::VCS_BRANCH_UPDATED.to_string(),
                            properties: Some(properties),
                            data: None,
                            location: None,
                        },
                    });
                    let mut guard = state.lock().unwrap();
                    guard.current = next;
                }
            }
        })
    }

    pub fn on_dispose(&self, directory: &str) {
        self.states.lock().unwrap().remove(directory);
    }

    pub async fn branch(
        &self,
        ctx: &crate::schema::ProjectInfo,
        directory: &str,
    ) -> Option<String> {
        let state = self.state_for(ctx, directory);
        let guard = state.lock().unwrap();
        guard.current.clone()
    }

    pub async fn default_branch(
        &self,
        ctx: &crate::schema::ProjectInfo,
        directory: &str,
    ) -> Option<String> {
        let state = self.state_for(ctx, directory);
        let guard = state.lock().unwrap();
        guard.root.as_ref().map(|root| root.name.clone())
    }

    pub async fn status(
        &self,
        ctx: &crate::schema::ProjectInfo,
        directory: &str,
        worktree: &str,
    ) -> Vec<VcsFileStatus> {
        if ctx.vcs.as_deref() != Some("git") {
            return Vec::new();
        }
        let has_head = self.git.has_head(directory).await;
        let r#ref = if has_head { Some("HEAD") } else { None };
        let list = self.git.status(directory).await;
        let stats = match r#ref {
            Some(r#ref) => self.git.stats(directory, r#ref).await,
            None => Vec::new(),
        };
        let map = nums(&stats);

        let mut items = list;
        items.sort_by(|a, b| a.file.cmp(&b.file));
        let mut result = Vec::new();
        for item in items {
            let stat = if let Some(stat) = map.get(&item.file) {
                Some(*stat)
            } else if item.status == Kind::Added {
                self.git
                    .stat_untracked(worktree, &item.file)
                    .await
                    .map(|s| (s.additions, s.deletions))
            } else {
                None
            };
            result.push(VcsFileStatus {
                file: item.file,
                additions: stat.map(|s| s.0).unwrap_or(0),
                deletions: stat.map(|s| s.1).unwrap_or(0),
                status: item.status.as_str().to_string(),
            });
        }
        result
    }

    pub async fn diff(
        &self,
        ctx: &crate::schema::ProjectInfo,
        directory: &str,
        mode: Mode,
        options: DiffOptions,
    ) -> Vec<VcsFileDiff> {
        if ctx.vcs.as_deref() != Some("git") {
            return Vec::new();
        }
        let state = self.state_for(ctx, directory);
        let has_head = self.git.has_head(directory).await;

        if mode == Mode::Git {
            let r#ref = if has_head { Some("HEAD") } else { None };
            return track(&self.git, directory, r#ref, options).await;
        }

        let (root, current) = {
            let guard = state.lock().unwrap();
            (guard.root.clone(), guard.current.clone())
        };
        let Some(root) = root else { return Vec::new() };
        if let Some(current) = &current {
            if current == &root.name {
                return Vec::new();
            }
        }
        let Some(r#ref) = self.git.merge_base(directory, &root.r#ref, "HEAD").await else {
            return Vec::new();
        };
        diff_against_ref(&self.git, directory, &r#ref, options).await
    }

    pub async fn diff_raw(&self, ctx: &crate::schema::ProjectInfo, directory: &str) -> String {
        if ctx.vcs.as_deref() != Some("git") {
            return String::new();
        }
        let has_head = self.git.has_head(directory).await;
        let status = self.git.status(directory).await;
        let tracked = if has_head {
            self.git.patch_all(directory, "HEAD", None).await.text
        } else {
            String::new()
        };
        let mut untracked = Vec::new();
        for item in &status {
            if item.code == "??" {
                untracked.push(
                    self.git
                        .patch_untracked(directory, &item.file, None)
                        .await
                        .text,
                );
            }
        }
        let mut parts: Vec<String> = Vec::new();
        if !tracked.is_empty() {
            parts.push(tracked);
        }
        parts.extend(untracked.into_iter().filter(|patch| !patch.is_empty()));
        parts.join("\n")
    }

    pub async fn apply(
        &self,
        ctx: &crate::schema::ProjectInfo,
        directory: &str,
        input: &VcsApplyInput,
    ) -> Result<VcsApplyResult, PatchApplyError> {
        if ctx.vcs.as_deref() != Some("git") {
            return Err(PatchApplyError::new(
                "Patch can't be applied because the project is not git-based",
                PatchApplyReason::NonGit,
            ));
        }
        let applied = self.git.apply_patch(directory, &input.patch).await;
        if applied.exit_code != 0 {
            return Err(PatchApplyError::new(
                "Patch can't be applied",
                PatchApplyReason::NotClean,
            ));
        }
        Ok(VcsApplyResult { applied: true })
    }

    /// Runs the reference's raw `{ code, text, stderr }` git helper for the
    /// caller-facing surface (currently unused by the service but mirrors the
    /// reference shape).
    pub async fn raw(&self, args: &[&str], cwd: &str) -> GitResult {
        let result = self
            .git
            .run(
                args,
                &crate::git::Options {
                    cwd: cwd.to_string(),
                    ..Default::default()
                },
            )
            .await;
        GitResult {
            code: result.exit_code,
            text: result.text(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quoted_path_unescapes() {
        assert_eq!(
            parse_quoted_path("\"a\\tb\"\"").map(|(v, _)| v),
            Some("a\tb".to_string())
        );
        assert_eq!(
            parse_quoted_path("\"a\\\"b\"").map(|(v, _)| v),
            Some("a\"b".to_string())
        );
        assert_eq!(parse_quoted_path("\"unterminated"), None);
        assert_eq!(
            parse_quoted_path("\"plain\"").map(|(v, end)| (v, end)),
            Some(("plain".to_string(), 7))
        );
    }

    #[test]
    fn parse_path_token_handles_quotes_and_tabs() {
        assert_eq!(parse_path_token("a\tb"), "a");
        assert_eq!(parse_path_token("\"a\\tb\""), "a\tb");
    }

    #[test]
    fn file_from_diff_path_strips_a_and_b_prefixes() {
        assert_eq!(
            file_from_diff_path(Some("a/src/index.ts")),
            Some("src/index.ts".to_string())
        );
        assert_eq!(
            file_from_diff_path(Some("b/src/index.ts")),
            Some("src/index.ts".to_string())
        );
        assert_eq!(
            file_from_diff_path(Some("src/index.ts")),
            Some("src/index.ts".to_string())
        );
        assert_eq!(file_from_diff_path(Some("/dev/null")), None);
        assert_eq!(file_from_diff_path(None), None);
    }

    #[test]
    fn file_from_git_header_handles_plain_and_quoted() {
        assert_eq!(
            file_from_git_header("a/src/index.ts b/src/index.ts"),
            Some("src/index.ts".to_string())
        );
        assert_eq!(
            file_from_git_header("\"a/file name.txt\" \"b/file name.txt\""),
            Some("file name.txt".to_string())
        );
        assert_eq!(file_from_git_header("no separator"), None);
    }

    #[test]
    fn file_from_patch_chunk_prefers_new_file() {
        let chunk = "diff --git a/a.ts b/a.ts\nnew file mode 100644\n--- /dev/null\n+++ b/a.ts\n";
        assert_eq!(file_from_patch_chunk(chunk), Some("a.ts".to_string()));
    }

    #[test]
    fn split_git_patch_splits_on_diff_headers() {
        let patch = Patch {
            text: "diff --git a/a.ts b/a.ts\n-a\n+b\ndiff --git a/b.ts b/b.ts\n-c\n+d".to_string(),
            truncated: false,
        };
        let chunks = split_git_patch(&patch);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with("diff --git a/a.ts"));
        assert!(chunks[1].starts_with("diff --git a/b.ts"));
    }

    #[test]
    fn split_git_patch_drops_last_chunk_when_truncated() {
        let patch = Patch {
            text: "diff --git a/a.ts b/a.ts\nx\ndiff --git a/b.ts b/b.ts\ny".to_string(),
            truncated: true,
        };
        assert_eq!(split_git_patch(&patch).len(), 1);
    }

    #[test]
    fn empty_patch_renders_headers() {
        assert_eq!(
            empty_patch("src/index.ts"),
            "--- src/index.ts\n+++ src/index.ts"
        );
    }
}
