//! Agent types.
//! From reference/packages/schema/src/agent.ts.

// TODO(integration): promote to oc-schema.
use crate::types::model::ModelRef;
use crate::types::permission::PermissionRule;
use crate::types::provider::ProviderRequest;

/// `Agent.Color` — a hex color or one of the theme color names.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum AgentColor {
    Named(AgentThemeColor),
    Hex(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentThemeColor {
    Primary,
    Secondary,
    Accent,
    Success,
    Warning,
    Error,
    Info,
}

/// `Agent.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    #[serde(default)]
    pub model: Option<ModelRef>,
    pub request: ProviderRequest,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub mode: AgentMode,
    pub hidden: bool,
    #[serde(default)]
    pub color: Option<AgentColor>,
    #[serde(default)]
    pub steps: Option<u64>,
    pub permissions: Vec<PermissionRule>,
}

/// `Agent.Info.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Subagent,
    Primary,
    All,
}
