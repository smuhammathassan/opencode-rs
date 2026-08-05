//! Skill types.
//! From reference/packages/schema/src/skill.ts.

use crate::types::schema::AbsolutePath;

/// `Skill.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub slash: Option<bool>,
    pub location: AbsolutePath,
    pub content: String,
}
