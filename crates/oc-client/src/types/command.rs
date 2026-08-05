//! Command types.
//! From reference/packages/schema/src/command.ts.

// TODO(integration): promote to oc-schema.
use crate::types::model::ModelRef;

/// `Command.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub name: String,
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub subtask: Option<bool>,
}
