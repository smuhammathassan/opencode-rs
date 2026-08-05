//! Project identifiers and VCS.
//! From reference/packages/core/src/project/schema.ts

use serde::{Deserialize, Serialize};

use crate::schema::AbsolutePath;

/// Re-export of `ProjectSchema.ID`.
pub use crate::ids::ProjectId as Id;

/// `ProjectSchema.Vcs` — `{ type: "git", store: AbsolutePath }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectVcs {
    pub r#type: String,
    pub store: AbsolutePath,
}

impl ProjectVcs {
    /// Build a git VCS descriptor for a git store directory.
    pub fn git(store: AbsolutePath) -> Self {
        ProjectVcs {
            r#type: "git".to_string(),
            store,
        }
    }
}
