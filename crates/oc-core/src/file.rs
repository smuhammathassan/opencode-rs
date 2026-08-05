//! File diff type.
//! From reference/packages/core/src/file.ts and
//! reference/packages/schema/src/revert.ts.

use serde::{Deserialize, Serialize};

use crate::schema::RelativePath;

/// `File.Diff` — `{ path, status, additions, deletions, patch }`.
/// From reference/packages/schema/src/revert.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: RelativePath,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub patch: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diff_json_shape() {
        let diff = FileDiff {
            path: RelativePath("src/main.rs".to_string()),
            status: "modified".to_string(),
            additions: 3,
            deletions: 1,
            patch: "@@ -1 +1 @@".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&diff).unwrap(),
            json!({ "path": "src/main.rs", "status": "modified", "additions": 3, "deletions": 1, "patch": "@@ -1 +1 @@" })
        );
    }
}
