//! From reference/packages/schema/src/revert.ts

use crate::schema::{NonNegativeInt, RelativePath};
use crate::session_message;
use serde::{Deserialize, Serialize};

/// `Revert.FileDiff` (`File.Diff`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileDiff {
    pub path: RelativePath,
    pub status: FileDiffStatus,
    pub additions: NonNegativeInt,
    pub deletions: NonNegativeInt,
    pub patch: String,
}

/// `File.Diff.status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FileDiffStatus {
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "modified")]
    Modified,
    #[serde(rename = "deleted")]
    Deleted,
}

/// `Revert.State`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct State {
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    #[serde(rename = "partID", skip_serializing_if = "Option::is_none", default)]
    pub part_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub files: Option<Vec<FileDiff>>,
}
