//! From reference/packages/schema/src/prompt.ts

use crate::schema::Finite;
use serde::{Deserialize, Serialize};

/// `Prompt.Source`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Source {
    pub start: Finite,
    pub end: Finite,
    pub text: String,
}

/// `Prompt.FileAttachment`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileAttachment {
    pub uri: String,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<Source>,
}

/// `Prompt.AgentAttachment`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentAttachment {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<Source>,
}

/// `Prompt`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prompt {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agents: Option<Vec<AgentAttachment>>,
}

/// `Prompt.fromUserMessage(input)`.
pub fn from_user_message(input: Prompt) -> Prompt {
    input
}
