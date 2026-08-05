//! Saved permission types.
//! From reference/packages/schema/src/permission-saved.ts.

// TODO(integration): promote to oc-schema.
/// `PermissionSaved.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSavedInfo {
    pub id: String,
    #[serde(rename = "projectID")]
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
