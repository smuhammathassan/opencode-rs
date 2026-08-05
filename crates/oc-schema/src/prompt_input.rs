//! From reference/packages/schema/src/prompt-input.ts

use crate::prompt::Source;
use serde::{Deserialize, Serialize};

/// `PromptInput.FileAttachment`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileAttachment {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<Source>,
}

/// `PromptInput.AgentAttachment` — reuses `Prompt.AgentAttachment`.
pub use crate::prompt::AgentAttachment;

/// `PromptInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prompt {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agents: Option<Vec<AgentAttachment>>,
}
