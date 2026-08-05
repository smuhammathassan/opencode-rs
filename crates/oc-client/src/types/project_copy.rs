//! Project copy types.
//! From reference/packages/schema/src/project-copy.ts.

use crate::types::location::LocationQueryRef;
use crate::types::schema::AbsolutePath;

/// `ProjectCopiesCreateInput` — the `projectID` and `sourceDirectory` are path
/// parameters and are omitted from the wire payload.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectCopyCreateInput {
    pub project_id: String,
    pub location: Option<LocationQueryRef>,
    pub strategy: String,
    pub directory: AbsolutePath,
    pub name: Option<String>,
}

/// `ProjectCopiesRemoveInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectCopyRemoveInput {
    pub project_id: String,
    pub location: Option<LocationQueryRef>,
    pub directory: AbsolutePath,
    pub force: bool,
}

/// `ProjectCopiesRefreshInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectCopyRefreshInput {
    pub project_id: String,
    pub location: Option<LocationQueryRef>,
}

/// `ProjectCopy.Copy` — the response of `projectCopy.create`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCopy {
    pub directory: AbsolutePath,
}
