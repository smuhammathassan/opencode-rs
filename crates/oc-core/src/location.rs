//! Location types.
//!
//! From reference/packages/schema/src/location.ts and
//! reference/packages/core/src/location.ts.

use serde::{Deserialize, Serialize};

use crate::ids::{ProjectId, WorkspaceId};
use crate::project::{ProjectVcs, Resolved};
use crate::schema::AbsolutePath;

/// `Location.Ref` — `{ directory, workspaceID? }`.
/// From reference/packages/schema/src/location.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationRef {
    pub directory: AbsolutePath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
}

impl LocationRef {
    /// Mirrors `Location.Ref.make(...)` (used in some call sites).
    pub fn make(directory: AbsolutePath, workspace_id: Option<WorkspaceId>) -> Self {
        LocationRef {
            directory,
            workspace_id,
        }
    }
}

/// `Location.Info` — `{ directory, workspaceID?, project: { id, directory } }`.
/// From reference/packages/schema/src/location.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationInfo {
    pub directory: AbsolutePath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    pub project: ProjectRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRef {
    pub id: ProjectId,
    pub directory: AbsolutePath,
}

/// `Location.response(data)` — `{ location: Info, data }`.
/// From reference/packages/schema/src/location.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response<T> {
    pub location: LocationInfo,
    pub data: T,
}

/// The resolved location service value — `Location.Info` plus the resolved
/// VCS.
/// From reference/packages/core/src/location.ts
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub directory: AbsolutePath,
    pub workspace_id: Option<WorkspaceId>,
    pub project: ProjectRef,
    pub vcs: Option<ProjectVcs>,
}

impl Location {
    /// Mirrors the `location(ref)` layer: resolves the project for a
    /// directory and produces a service value.
    pub fn from_resolved(
        directory: AbsolutePath,
        workspace_id: Option<WorkspaceId>,
        resolved: Resolved,
    ) -> Self {
        Location {
            directory,
            workspace_id,
            project: ProjectRef {
                id: resolved.id,
                directory: resolved.directory,
            },
            vcs: resolved.vcs,
        }
    }
}
