//! From reference/packages/schema/src/llm.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `LLM.ProviderMetadata` — `Record<String, Record<String, Unknown>>`.
pub type ProviderMetadata = IndexMap<String, IndexMap<String, Value>>;

/// `LLM.ToolTextContent`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolTextContent {
    #[serde(rename = "type")]
    pub r#type: ToolTextContentType,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolTextContentType {
    #[serde(rename = "text")]
    Value,
}

/// `LLM.ToolFileContent`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolFileContent {
    #[serde(rename = "type")]
    pub r#type: ToolFileContentType,
    pub uri: String,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolFileContentType {
    #[serde(rename = "file")]
    Value,
}

/// `LLM.ToolContent` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ToolContent {
    Text(ToolTextContent),
    File(ToolFileContent),
}
