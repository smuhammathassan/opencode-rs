//! Saved permission types.
//! From reference/packages/schema/src/permission-saved.ts.

/// `PermissionSaved.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSavedInfo {
    pub id: String,
    pub project_id: String,
    pub action: String,
    pub resource: String,
}

/// `PermissionsListSavedInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionsListSavedInput {
    pub project_id: Option<String>,
}

/// `PermissionsRemoveSavedInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionsRemoveSavedInput {
    pub id: String,
}
