//! Location types.
//! From reference/packages/schema/src/location.ts.

// TODO(integration): promote to oc-schema.
use crate::types::schema::AbsolutePath;

/// `Location.Ref` — a location reference.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationRef {
    pub directory: AbsolutePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "workspaceID")]
    pub workspace_id: Option<String>,
}

/// `Location.Info` — a resolved location including its project.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub directory: AbsolutePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "workspaceID")]
    pub workspace_id: Option<String>,
    pub project: ProjectRef,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub id: String,
    pub directory: AbsolutePath,
}

/// The `location` query parameter accepted by location-scoped endpoints.
/// Encoded as `location[directory]=...&location[workspace]=...`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationQueryRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<AbsolutePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

/// Input shared by location-scoped list endpoints: `{ location?: LocationQueryRef }`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocationInput {
    pub location: Option<LocationQueryRef>,
}

/// A location-scoped response: `{ location: LocationInfo, data: T }`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationData<T> {
    pub location: LocationInfo,
    pub data: T,
}
