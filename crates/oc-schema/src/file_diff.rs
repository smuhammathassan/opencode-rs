//! From reference/packages/schema/src/file-diff.ts

use crate::schema::Finite;
use serde::{Deserialize, Serialize};

/// `FileDiff.Info` (`SnapshotFileDiff`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub patch: Option<String>,
    pub additions: Finite,
    pub deletions: Finite,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<Status>,
}

/// `FileDiff.Info.status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "deleted")]
    Deleted,
    #[serde(rename = "modified")]
    Modified,
}
