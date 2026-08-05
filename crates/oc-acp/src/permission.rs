//! Permission request handling.
//!
//! From reference/packages/opencode/src/acp/permission.ts. Bridges opencode
//! `permission.asked` events to ACP `session/request_permission` notifications
//! and replies on the opencode SDK.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::Mutex;

use crate::connection::AgentSideConnection;
use crate::sdk::{OpencodeClient, PermissionAskedProperties, ToolStateRunning};
use crate::session::Service as SessionService;
use crate::tool;
use crate::types::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, ToolCallContent, ToolCallLocation,
    ToolCallUpdate, WriteTextFileRequest,
};

/// The permission options offered to the client.
fn permission_options() -> Vec<PermissionOption> {
    vec![
        PermissionOption {
            option_id: "once".into(),
            kind: PermissionOptionKind::AllowOnce,
            name: "Allow once".into(),
        },
        PermissionOption {
            option_id: "always".into(),
            kind: PermissionOptionKind::AllowAlways,
            name: "Always allow".into(),
        },
        PermissionOption {
            option_id: "reject".into(),
            kind: PermissionOptionKind::RejectOnce,
            name: "Reject".into(),
        },
    ]
}

/// A permission reply sent back to opencode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    Once,
    Always,
    Reject,
}

/// Handles `permission.asked` events for a subscription.
pub struct Handler {
    sdk: Arc<dyn OpencodeClient>,
    connection: Option<Arc<dyn AgentSideConnection>>,
    session: Arc<SessionService>,
    queues: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Handler {
    pub fn new(
        sdk: Arc<dyn OpencodeClient>,
        connection: Option<Arc<dyn AgentSideConnection>>,
        session: Arc<SessionService>,
    ) -> Self {
        Self {
            sdk,
            connection,
            session,
            queues: Mutex::new(HashMap::new()),
        }
    }

    /// `handle` from reference/packages/opencode/src/acp/permission.ts.
    ///
    /// Serializes processing per session by chaining onto a per-session queue.
    pub async fn handle(&self, permission: &PermissionAskedProperties) {
        let lock = {
            let mut queues = self.queues.lock().await;
            queues
                .entry(permission.session_id.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        let _ = self.process(permission).await;
    }

    /// `process` from reference/packages/opencode/src/acp/permission.ts.
    async fn process(&self, permission: &PermissionAskedProperties) {
        let Some(session) = self.session.try_get(&permission.session_id).await else {
            return;
        };

        let supports_permission = self
            .connection
            .as_ref()
            .is_some_and(|connection| connection.supports_request_permission());
        if !supports_permission {
            self.reply(&permission.id, Reply::Reject, &session.cwd)
                .await;
            return;
        }
        let connection = self.connection.as_ref().unwrap().clone();

        let tool_call_id = permission
            .tool
            .as_ref()
            .map(|tool| tool.call_id.clone())
            .unwrap_or_else(|| permission.id.clone());
        let tool_call =
            permission_tool_call(&tool_call_id, &permission.permission, &permission.metadata).await;

        let result = connection
            .request_permission(RequestPermissionRequest {
                session_id: permission.session_id.clone(),
                tool_call,
                options: permission_options(),
            })
            .await;
        let result = match result {
            Ok(result) => Some(result),
            Err(()) => {
                self.reply(&permission.id, Reply::Reject, &session.cwd)
                    .await;
                None
            }
        };
        let Some(result) = result else {
            return;
        };

        let reply = selected_reply(&result);
        if reply != Reply::Once && reply != Reply::Always {
            self.reply(&permission.id, Reply::Reject, &session.cwd)
                .await;
            return;
        }

        if permission.permission == "edit" {
            self.write_proposed_edit(&session.id, &permission.metadata)
                .await;
        }

        self.reply(&permission.id, reply, &session.cwd).await;
    }

    /// `reply` from reference/packages/opencode/src/acp/permission.ts.
    async fn reply(&self, request_id: &str, reply: Reply, directory: &str) {
        let reply = match reply {
            Reply::Once => "once",
            Reply::Always => "always",
            Reply::Reject => "reject",
        };
        let _ = self
            .sdk
            .permission_reply(request_id, reply, directory)
            .await;
    }

    /// `writeProposedEdit` from reference/packages/opencode/src/acp/permission.ts.
    async fn write_proposed_edit(&self, session_id: &str, metadata: &ToolInput) {
        let filepath = string_value(metadata.get("filepath"));
        let diff = string_value(metadata.get("diff"));
        let (Some(filepath), Some(diff)) = (filepath, diff) else {
            return;
        };
        let supports_write = self
            .connection
            .as_ref()
            .is_some_and(|connection| connection.supports_write_text_file());
        let Some(connection) = self.connection.as_ref() else {
            return;
        };
        if !supports_write {
            return;
        }

        let content = if std::path::Path::new(&filepath).exists() {
            std::fs::read_to_string(&filepath).unwrap_or_default()
        } else {
            String::new()
        };
        let Some(next) = apply_patch(&content, &diff) else {
            return;
        };

        let _ = connection
            .write_text_file(WriteTextFileRequest {
                session_id: session_id.to_string(),
                path: filepath,
                content: next,
            })
            .await;
    }
}

/// The raw tool input for a permission request.
pub type ToolInput = Map<String, Value>;

/// `permissionToolCall` from reference/packages/opencode/src/acp/permission.ts.
///
/// Note: the reference spreads `pendingToolCall` (ordering `toolCallId`, `title`,
/// `kind`, `status`, `locations`, `rawInput`) and appends `content`. The
/// `ToolCallUpdate` struct reorders these to match the `tool_call_update` session
/// notification layout; JSON object key order is not semantically meaningful.
async fn permission_tool_call(
    tool_call_id: &str,
    tool_name: &str,
    input: &ToolInput,
) -> ToolCallUpdate {
    let state = ToolStateRunning {
        input: input.clone(),
        title: permission_title(tool_name, input),
        metadata: None,
    };
    let tool_call = tool::pending_tool_call(tool::PendingToolCallInput {
        tool_call_id: tool_call_id.to_string(),
        tool_name,
        state: tool::RunningToolInput {
            input: state.input.clone(),
            title: state.title.clone(),
        },
        cwd: None,
    });
    let content = permission_content(tool_name, input).await;
    let locations = permission_locations(tool_name, input);
    ToolCallUpdate {
        tool_call_id: tool_call.tool_call_id,
        status: tool_call.status,
        kind: Some(tool_call.kind),
        title: Some(tool_call.title),
        locations: Some(locations),
        raw_input: Some(tool_call.raw_input),
        content: if content.is_empty() {
            None
        } else {
            Some(content)
        },
        raw_output: None,
    }
}

/// `permissionTitle` from reference/packages/opencode/src/acp/permission.ts.
fn permission_title(tool_name: &str, input: &ToolInput) -> Option<String> {
    match tool_name.to_lowercase().as_str() {
        "external_directory" => string_value(input.get("description"))
            .or_else(|| string_value(input.get("command")))
            .or_else(|| string_value(input.get("parentDir"))),
        "webfetch" => string_value(input.get("url")),
        "websearch" => string_value(input.get("query")),
        "grep" | "glob" => string_value(input.get("pattern")),
        "read" | "edit" | "write" => edit_title(input),
        _ => None,
    }
}

/// `editTitle` from reference/packages/opencode/src/acp/permission.ts.
fn edit_title(input: &ToolInput) -> Option<String> {
    let files = file_metadata(input);
    if files.len() == 1 {
        return files[0]
            .relative_path
            .clone()
            .or_else(|| Some(files[0].file_path.clone()));
    }
    if files.len() > 1 {
        return Some(format!("{} files", files.len()));
    }
    string_value(input.get("filePath"))
        .or_else(|| string_value(input.get("filepath")))
        .or_else(|| string_value(input.get("path")))
}

/// `permissionLocations` from reference/packages/opencode/src/acp/permission.ts.
fn permission_locations(tool_name: &str, input: &ToolInput) -> Vec<ToolCallLocation> {
    let files = file_metadata(input);
    if !files.is_empty() {
        let mut seen = std::collections::HashSet::new();
        let mut locations = Vec::new();
        for file in files {
            let mut paths = vec![file.file_path];
            if let Some(move_path) = file.move_path {
                paths.push(move_path);
            }
            for path in paths {
                if seen.insert(path.clone()) {
                    locations.push(ToolCallLocation { path });
                }
            }
        }
        return locations;
    }
    tool::to_locations(tool_name, input, None)
}

/// `permissionContent` from reference/packages/opencode/src/acp/permission.ts.
async fn permission_content(tool_name: &str, input: &ToolInput) -> Vec<ToolCallContent> {
    if tool_name.to_lowercase() != "edit" {
        return Vec::new();
    }

    let files = file_metadata(input);
    if !files.is_empty() {
        return diff_content_for_files(&files).await;
    }

    let filepath =
        string_value(input.get("filepath")).or_else(|| string_value(input.get("filePath")));
    let diff = string_value(input.get("diff"));
    let (Some(filepath), Some(diff)) = (filepath, diff) else {
        return Vec::new();
    };
    match diff_content_for_patch(&filepath, &diff, None).await {
        Some(content) => vec![content],
        None => Vec::new(),
    }
}

/// `diffContentForFiles` from reference/packages/opencode/src/acp/permission.ts.
async fn diff_content_for_files(files: &[PermissionFileMetadata]) -> Vec<ToolCallContent> {
    let mut content = Vec::new();
    for file in files {
        let Some(patch) = &file.patch else {
            continue;
        };
        if let Some(diff) =
            diff_content_for_patch(&file.file_path, patch, file.move_path.as_deref()).await
        {
            content.push(diff);
        }
    }
    content
}

/// `diffContentForPatch` from reference/packages/opencode/src/acp/permission.ts.
async fn diff_content_for_patch(
    filepath: &str,
    diff: &str,
    display_path: Option<&str>,
) -> Option<ToolCallContent> {
    let content = if std::path::Path::new(filepath).exists() {
        std::fs::read_to_string(filepath).unwrap_or_default()
    } else {
        String::new()
    };
    let next = apply_patch(&content, diff)?;
    Some(ToolCallContent::Diff(crate::types::Diff {
        path: display_path
            .map(str::to_string)
            .unwrap_or_else(|| filepath.to_string()),
        old_text: Some(content),
        new_text: next,
    }))
}

/// `selectedReply` from reference/packages/opencode/src/acp/permission.ts.
fn selected_reply(result: &RequestPermissionResponse) -> Reply {
    match &result.outcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id }) => {
            match option_id.as_str() {
                "once" => Reply::Once,
                "always" => Reply::Always,
                _ => Reply::Reject,
            }
        }
        RequestPermissionOutcome::Cancelled(_) => Reply::Reject,
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// `PermissionFileMetadata` from reference/packages/opencode/src/acp/permission.ts.
#[derive(Debug, Clone, Default)]
struct PermissionFileMetadata {
    file_path: String,
    relative_path: Option<String>,
    move_path: Option<String>,
    patch: Option<String>,
}

/// `fileMetadata` from reference/packages/opencode/src/acp/permission.ts.
fn file_metadata(input: &ToolInput) -> Vec<PermissionFileMetadata> {
    let Some(Value::Array(files)) = input.get("files") else {
        return Vec::new();
    };
    files
        .iter()
        .filter_map(|file| {
            let info = file.as_object()?;
            let file_path = string_value(info.get("filePath"))?;
            Some(PermissionFileMetadata {
                file_path,
                relative_path: string_value(info.get("relativePath")),
                move_path: string_value(info.get("movePath")),
                patch: string_value(info.get("patch")),
            })
        })
        .collect()
}

/// `applyPatch` from the `diff` npm package.
///
/// Applies a unified diff to `original`, returning `None` on failure. Supports
/// `---`/`+++` headers, `@@ -start,count +start,count @@` hunks, context,
/// addition and deletion lines, and `\ No newline at end of file` markers.
///
/// TODO(integration): this is a best-effort subset of the `diff` package's
/// `applyPatch`; edge cases (partial hunks, offset tolerance) may diverge.
pub fn apply_patch(original: &str, patch: &str) -> Option<String> {
    let hunks = parse_patch(patch)?;
    if hunks.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
    // `lines()` drops a trailing empty line; track whether the original ended
    // with a newline so insertion at EOF works like the diff package.
    let mut offset: isize = 0;
    for hunk in hunks {
        // `@@ -start,...` is 1-based in unified diff; the diff package applies
        // at `oldStart - 1` plus the cumulative net line change of prior hunks.
        let mut position = (hunk.start.saturating_sub(1)) as isize + offset;
        for line in &hunk.lines {
            let base = position.max(0) as usize;
            match line.op {
                Op::Context => {
                    if lines.get(base).map(String::as_str) != Some(line.text.as_str()) {
                        return None;
                    }
                    position += 1;
                }
                Op::Remove => {
                    if lines.get(base).map(String::as_str) != Some(line.text.as_str()) {
                        return None;
                    }
                    lines.remove(base);
                }
                Op::Add => {
                    lines.insert(base, line.text.clone());
                    position += 1;
                }
            }
        }
        let old_lines = hunk.lines.iter().filter(|line| line.op != Op::Add).count() as isize;
        let new_lines = hunk
            .lines
            .iter()
            .filter(|line| line.op != Op::Remove)
            .count() as isize;
        offset += new_lines - old_lines;
    }
    let mut result = lines.join("\n");
    if original.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Context,
    Remove,
    Add,
}

#[derive(Debug)]
struct PatchLine {
    op: Op,
    text: String,
}

#[derive(Debug)]
struct Hunk {
    start: usize,
    lines: Vec<PatchLine>,
}

/// Parse a unified diff into ordered hunks.
fn parse_patch(patch: &str) -> Option<Vec<Hunk>> {
    let mut hunks = Vec::new();
    let mut current: Option<Hunk> = None;
    let mut no_newline_next = false;

    for raw in patch.lines() {
        let line = if raw.strip_prefix('\\').is_some() {
            // `\ No newline at end of file`
            if no_newline_next {
                if let Some(hunk) = &mut current {
                    if let Some(last) = hunk.lines.last_mut() {
                        last.text.push('\n');
                    }
                }
            }
            continue;
        } else {
            raw
        };
        no_newline_next = line.starts_with("-") || line.starts_with("+");

        if let Some(header) = line.strip_prefix("@@") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = Some(parse_hunk_header(header)?);
            continue;
        }
        let Some(hunk) = current.as_mut() else {
            continue;
        };
        if let Some(text) = line.strip_prefix('+') {
            hunk.lines.push(PatchLine {
                op: Op::Add,
                text: text.to_string(),
            });
        } else if let Some(text) = line.strip_prefix('-') {
            hunk.lines.push(PatchLine {
                op: Op::Remove,
                text: text.to_string(),
            });
        } else if let Some(text) = line.strip_prefix(' ') {
            hunk.lines.push(PatchLine {
                op: Op::Context,
                text: text.to_string(),
            });
        }
    }
    if let Some(hunk) = current.take() {
        hunks.push(hunk);
    }
    Some(hunks)
}

/// Parse ` -start,count +start,count ...` (after `@@`).
fn parse_hunk_header(header: &str) -> Option<Hunk> {
    let rest = header.trim_end_matches("@@");
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let old = parts.first()?.trim_start_matches('-');
    let start = old.split(',').next()?.parse::<usize>().ok()?;
    Some(Hunk {
        start,
        lines: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_simple_patch() {
        let original = "line1\nline2\nline3\n";
        let patch = "--- a\n+++ b\n@@ -1,3 +1,3 @@\n line1\n-line2\n+changed\n line3\n";
        assert_eq!(
            apply_patch(original, patch).as_deref(),
            Some("line1\nchanged\nline3\n")
        );
    }

    #[test]
    fn apply_patch_with_insert() {
        let original = "a\nc\n";
        let patch = "@@ -1,2 +1,3 @@\n a\n+b\n c\n";
        assert_eq!(apply_patch(original, patch).as_deref(), Some("a\nb\nc\n"));
    }

    #[test]
    fn apply_patch_mismatch_returns_none() {
        let original = "a\nb\n";
        let patch = "@@ -1,2 +1,2 @@\n x\n y\n";
        assert_eq!(apply_patch(original, patch), None);
    }

    #[test]
    fn selected_reply_handles_outcomes() {
        let cancelled = RequestPermissionResponse {
            outcome: RequestPermissionOutcome::Cancelled(crate::types::CancelledOutcome {}),
        };
        assert_eq!(selected_reply(&cancelled), Reply::Reject);
        let selected = RequestPermissionResponse {
            outcome: RequestPermissionOutcome::Selected(SelectedPermissionOutcome {
                option_id: "always".into(),
            }),
        };
        assert_eq!(selected_reply(&selected), Reply::Always);
    }

    #[test]
    fn file_metadata_parses_files() {
        let mut input = ToolInput::new();
        input.insert(
            "files".into(),
            Value::Array(vec![serde_json::json!({
                "filePath": "/a/b.rs",
                "relativePath": "b.rs",
                "patch": "diff"
            })]),
        );
        let files = file_metadata(&input);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_path, "/a/b.rs");
        assert_eq!(files[0].relative_path.as_deref(), Some("b.rs"));
    }
}
