//! From reference/packages/schema/src/permission-saved.ts

use crate::identifier::ascending;
use crate::project_id::ProjectID;
use serde::{Deserialize, Serialize};

/// `PermissionSaved.ID`.
pub type ID = String;

/// `PermissionSaved.ID.create()`.
pub fn create_id() -> ID {
    format!("psv_{}", ascending())
}

/// `PermissionSaved.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    #[serde(rename = "projectID")]
    pub project_id: ProjectID,
    pub action: String,
    pub resource: String,
}
