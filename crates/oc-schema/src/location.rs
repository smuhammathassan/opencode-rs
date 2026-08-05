//! From reference/packages/schema/src/location.ts

use crate::project_id::ProjectID;
use crate::schema::AbsolutePath;
use crate::workspace_id::WorkspaceID;
use serde::{Deserialize, Serialize};

/// `Location.Ref`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Ref {
    pub directory: AbsolutePath,
    #[serde(
        rename = "workspaceID",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub workspace_id: Option<WorkspaceID>,
}

/// `Location.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub directory: AbsolutePath,
    #[serde(
        rename = "workspaceID",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub workspace_id: Option<WorkspaceID>,
    pub project: Project,
}

/// The nested `project` property of `Location.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: ProjectID,
    pub directory: AbsolutePath,
}

/// `Location.response(data)` — `Schema.Struct({ location: Info, data })`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Response<T> {
    pub location: Info,
    pub data: T,
}
