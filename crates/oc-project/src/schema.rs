//! Data shapes mirrored from `@opencode-ai/schema` (project.ts, worktree-event.ts,
//! vcs-event.ts, file-diff.ts) plus the service-level shapes defined in
//! `src/project/{project,vcs}.ts` and `src/{worktree,snapshot}`.
//!
//! TODO(integration): promote these types to oc-schema once it is implemented;
//! they are private mirrors required because oc-schema is still a stub.

#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

/// From reference/packages/schema/src/project-id.ts
/// Branded string; `ID.global` is `"global"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct ProjectID(pub String);

impl ProjectID {
    pub fn global() -> ProjectID {
        ProjectID("global".to_string())
    }

    pub fn make(value: impl Into<String>) -> ProjectID {
        ProjectID(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// From reference/packages/schema/src/project.ts
pub const PROJECT_VCS_GIT: &str = "git";

/// `Project.Icon`: `{ url?, override?, color? }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectIcon {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "override", skip_serializing_if = "Option::is_none")]
    pub override_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// `Project.Commands`: `{ start? }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCommands {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
}

/// `Project.Time`: `{ created, updated, initialized? }` (NonNegativeInt).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTime {
    pub created: u64,
    pub updated: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialized: Option<u64>,
}

/// `Project.Info` (identifier `Project`). Field order matches the zod struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: ProjectID,
    pub worktree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<ProjectCommands>,
    pub time: ProjectTime,
    pub sandboxes: Vec<String>,
}

/// `Project.update` input from reference/packages/opencode/src/project/project.ts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectUpdateInput {
    pub projectID: ProjectID,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<ProjectCommands>,
}

/// `Project.update` payload schema (identifier `ProjectUpdateInput`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectUpdatePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<ProjectCommands>,
}

/// `Project.NotFoundError` tagged error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectNotFoundError {
    #[serde(rename = "_tag")]
    pub tag: String,
    pub projectID: ProjectID,
}

impl ProjectNotFoundError {
    pub fn new(projectID: ProjectID) -> Self {
        ProjectNotFoundError { tag: "Project.NotFoundError".to_string(), projectID }
    }
}

// ---------------------------------------------------------------------------
// Vcs (reference/packages/opencode/src/project/vcs.ts)
// ---------------------------------------------------------------------------

/// `Vcs.Info` (identifier `VcsInfo`): `{ branch?, default_branch? }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
}

pub type VcsFileStatusKind = String;

/// `Vcs.FileDiff` (identifier `VcsFileDiff`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsFileDiff {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// `Vcs.FileStatus` (identifier `VcsFileStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsFileStatus {
    pub file: String,
    pub additions: u64,
    pub deletions: u64,
    pub status: String,
}

/// `Vcs.ApplyInput`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsApplyInput {
    pub patch: String,
}

/// `Vcs.ApplyResult`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsApplyResult {
    pub applied: bool,
}

/// `Vcs.PatchApplyError` (identifier `VcsPatchApplyError`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchApplyError {
    #[serde(rename = "_tag")]
    pub tag: String,
    pub message: String,
    pub reason: PatchApplyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PatchApplyReason {
    #[serde(rename = "non-git")]
    NonGit,
    #[serde(rename = "not-clean")]
    NotClean,
}

impl PatchApplyError {
    pub fn new(message: String, reason: PatchApplyReason) -> Self {
        PatchApplyError { tag: "VcsPatchApplyError".to_string(), message, reason }
    }
}

/// `Vcs.Mode`: `"git" | "branch"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsMode {
    Git,
    Branch,
}

// ---------------------------------------------------------------------------
// Worktree (reference/packages/opencode/src/worktree/index.ts)
// ---------------------------------------------------------------------------

/// `Worktree.Info` (identifier `Worktree`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub directory: String,
}

/// `Worktree.CreateInput` (identifier `WorktreeCreateInput`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorktreeCreateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startCommand: Option<String>,
}

/// `Worktree.RemoveInput` (identifier `WorktreeRemoveInput`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRemoveInput {
    pub directory: String,
}

/// `Worktree.ResetInput` (identifier `WorktreeResetInput`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeResetInput {
    pub directory: String,
}

/// A tagged error carrying `{ _tag, message }`, matching the reference's
/// `Schema.TaggedErrorClass` shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaggedError {
    #[serde(rename = "_tag")]
    pub tag: String,
    pub message: String,
}

impl TaggedError {
    pub fn new(tag: impl Into<String>, message: impl Into<String>) -> Self {
        TaggedError { tag: tag.into(), message: message.into() }
    }
}

macro_rules! worktree_error {
    ($variant:ident, $fn:ident, $tag:literal) => {
        #[allow(missing_docs)]
        pub fn $fn(message: impl Into<String>) -> WorktreeError {
            WorktreeError::$variant(TaggedError::new($tag, message))
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeError {
    NotGit(TaggedError),
    NameGenerationFailed(TaggedError),
    CreateFailed(TaggedError),
    StartCommandFailed(TaggedError),
    RemoveFailed(TaggedError),
    ResetFailed(TaggedError),
    ListFailed(TaggedError),
}

impl WorktreeError {
    worktree_error!(NotGit, not_git, "WorktreeNotGitError");
    worktree_error!(NameGenerationFailed, name_generation_failed, "WorktreeNameGenerationFailedError");
    worktree_error!(CreateFailed, create_failed, "WorktreeCreateFailedError");
    worktree_error!(StartCommandFailed, start_command_failed, "WorktreeStartCommandFailedError");
    worktree_error!(RemoveFailed, remove_failed, "WorktreeRemoveFailedError");
    worktree_error!(ResetFailed, reset_failed, "WorktreeResetFailedError");
    worktree_error!(ListFailed, list_failed, "WorktreeListFailedError");

    pub fn tag(&self) -> &str {
        match self {
            WorktreeError::NotGit(e)
            | WorktreeError::NameGenerationFailed(e)
            | WorktreeError::CreateFailed(e)
            | WorktreeError::StartCommandFailed(e)
            | WorktreeError::RemoveFailed(e)
            | WorktreeError::ResetFailed(e)
            | WorktreeError::ListFailed(e) => &e.tag,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            WorktreeError::NotGit(e)
            | WorktreeError::NameGenerationFailed(e)
            | WorktreeError::CreateFailed(e)
            | WorktreeError::StartCommandFailed(e)
            | WorktreeError::RemoveFailed(e)
            | WorktreeError::ResetFailed(e)
            | WorktreeError::ListFailed(e) => &e.message,
        }
    }
}

impl std::fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.tag(), self.message())
    }
}

impl std::error::Error for WorktreeError {}

// ---------------------------------------------------------------------------
// Snapshot (reference/packages/opencode/src/snapshot/index.ts)
// ---------------------------------------------------------------------------

/// `Snapshot.Patch`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotPatch {
    pub hash: String,
    pub files: Vec<String>,
}

/// `Snapshot.FileDiff` (identifier `SnapshotFileDiff`, from
/// reference/packages/schema/src/file-diff.ts). `file` is optional in the
/// schema; the producer always populates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFileDiff {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// Events (reference/packages/schema/src/{worktree-event,vcs-event}.ts)
// ---------------------------------------------------------------------------

/// `WorktreeEvent.Ready` payload (type `worktree.ready`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeReady {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// `WorktreeEvent.Failed` payload (type `worktree.failed`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeFailed {
    pub message: String,
}

/// `VcsEvent.BranchUpdated` payload (type `vcs.branch.updated`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VcsBranchUpdated {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// `Project.Event.Updated` (type `project.updated`); properties are the Info.
pub const PROJECT_UPDATED: &str = "project.updated";
pub const WORKTREE_READY: &str = "worktree.ready";
pub const WORKTREE_FAILED: &str = "worktree.failed";
pub const VCS_BRANCH_UPDATED: &str = "vcs.branch.updated";
pub const COMMAND_EXECUTED: &str = "command.executed";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_info_serializes_omitting_absent_optionals() {
        let info = ProjectInfo {
            id: ProjectID::global(),
            worktree: "/worktree".to_string(),
            vcs: Some("git".to_string()),
            name: None,
            icon: None,
            commands: None,
            time: ProjectTime { created: 1, updated: 2, initialized: None },
            sandboxes: vec!["/a".to_string(), "/b".to_string()],
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "global",
                "worktree": "/worktree",
                "vcs": "git",
                "time": { "created": 1, "updated": 2 },
                "sandboxes": ["/a", "/b"],
            })
        );
    }

    #[test]
    fn project_icon_uses_override_key() {
        let icon = ProjectIcon { url: None, override_: Some("x".to_string()), color: Some("red".to_string()) };
        let json = serde_json::to_value(&icon).unwrap();
        assert_eq!(json, serde_json::json!({ "override": "x", "color": "red" }));
    }

    #[test]
    fn snapshot_patch_golden() {
        let patch = SnapshotPatch { hash: "abc123".to_string(), files: vec!["/w/a.ts".to_string()] };
        let json = serde_json::to_value(&patch).unwrap();
        assert_eq!(json, serde_json::json!({ "hash": "abc123", "files": ["/w/a.ts"] }));
    }

    #[test]
    fn worktree_info_golden() {
        let info = WorktreeInfo {
            name: "fix-thing".to_string(),
            branch: Some("opencode/fix-thing".to_string()),
            directory: "/data/worktree/pid/fix-thing".to_string(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "name": "fix-thing",
                "branch": "opencode/fix-thing",
                "directory": "/data/worktree/pid/fix-thing",
            })
        );
    }

    #[test]
    fn worktree_create_input_golden() {
        let input = WorktreeCreateInput { name: Some("x".to_string()), startCommand: None };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json, serde_json::json!({ "name": "x" }));
    }

    #[test]
    fn tagged_errors_serialize_with_tag() {
        let error = WorktreeError::not_git("Worktrees are only supported for git projects");
        assert_eq!(
            serde_json::to_value(error.tag()).unwrap(),
            serde_json::json!("WorktreeNotGitError")
        );
        let inner: &TaggedError = match &error {
            WorktreeError::NotGit(inner) => inner,
            _ => unreachable!(),
        };
        let json = serde_json::to_value(inner).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "_tag": "WorktreeNotGitError",
                "message": "Worktrees are only supported for git projects",
            })
        );
    }

    #[test]
    fn vcs_file_status_golden() {
        let status = VcsFileStatus { file: "a.ts".to_string(), additions: 3, deletions: 1, status: "modified".to_string() };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "file": "a.ts", "additions": 3, "deletions": 1, "status": "modified" })
        );
    }
}
