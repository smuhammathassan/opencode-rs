//! PTY types.
//! From reference/packages/schema/src/pty.ts.

// TODO(integration): promote to oc-schema.
use crate::types::location::LocationQueryRef;
use std::collections::HashMap;

/// `Pty.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyInfo {
    pub id: String,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: PtyStatus,
    pub pid: u64,
    #[serde(default)]
    pub exit_code: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PtyStatus {
    Running,
    Exited,
}

/// `PtysCreateInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyCreateInput {
    pub location: Option<LocationQueryRef>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub env: Option<HashMap<String, String>>,
}

/// `PtysGetInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyGetInput {
    pub pty_id: String,
    pub location: Option<LocationQueryRef>,
}

/// `PtysUpdateInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyUpdateInput {
    pub pty_id: String,
    pub location: Option<LocationQueryRef>,
    pub title: Option<String>,
    pub size: Option<PtySize>,
}

/// `PtysRemoveInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyRemoveInput {
    pub pty_id: String,
    pub location: Option<LocationQueryRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtySize {
    pub rows: u64,
    pub cols: u64,
}
