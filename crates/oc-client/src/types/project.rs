//! Project types.
//! From reference/packages/schema/src/project.ts.

// TODO(integration): promote to oc-schema.
/// `Project.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub worktree: String,
    #[serde(default)]
    pub vcs: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<serde_json::Value>,
    #[serde(default)]
    pub commands: Option<serde_json::Value>,
    pub time: ProjectTime,
    pub sandboxes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTime {
    pub created: u64,
    pub updated: u64,
    #[serde(default)]
    pub initialized: Option<u64>,
}
