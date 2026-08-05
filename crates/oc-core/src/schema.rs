//! Path and scalar schema helpers.
//!
//! From reference/packages/core/src/schema.ts and
//! reference/packages/schema/src/schema.ts.
//!
//! The reference brands these with `effect` Schema brands; in Rust they are
//! transparent newtypes so serialized JSON stays a plain string.

use serde::{Deserialize, Serialize};

/// An absolute filesystem path (branded `AbsolutePath` in the reference).
/// From reference/packages/schema/src/schema.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AbsolutePath(pub String);

impl AbsolutePath {
    /// Mirrors `AbsolutePath.make(...)`.
    pub fn make(value: impl Into<String>) -> Self {
        AbsolutePath(value.into())
    }
}

impl From<&str> for AbsolutePath {
    fn from(value: &str) -> Self {
        AbsolutePath(value.to_string())
    }
}

impl From<String> for AbsolutePath {
    fn from(value: String) -> Self {
        AbsolutePath(value)
    }
}

impl std::fmt::Display for AbsolutePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A path relative to a project root (branded `RelativePath` in the reference).
/// From reference/packages/schema/src/schema.ts
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelativePath(pub String);

impl RelativePath {
    /// Mirrors `RelativePath.make(...)`.
    pub fn make(value: impl Into<String>) -> Self {
        RelativePath(value.into())
    }
}

impl From<&str> for RelativePath {
    fn from(value: &str) -> Self {
        RelativePath(value.to_string())
    }
}

impl From<String> for RelativePath {
    fn from(value: String) -> Self {
        RelativePath(value)
    }
}

impl std::fmt::Display for RelativePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stand-in for `Types.DeepMutable`. Rust types have no readonly distinction,
/// so this is a marker trait retained for parity with the reference's
/// `core/schema.ts`.
pub trait DeepMutable {}

/// Strip a trailing newline pair, as `path.resolve` output trimming does in
/// `git.ts`'s `resolvePath`.
pub(crate) fn trim_newlines(value: &str) -> &str {
    value.trim_end_matches(['\r', '\n'])
}
