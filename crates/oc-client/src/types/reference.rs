//! Reference types.
//! From reference/packages/schema/src/reference.ts.

use crate::types::schema::AbsolutePath;

/// `Reference.Source` — tagged on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ReferenceSource {
    #[serde(rename = "local")]
    Local {
        path: AbsolutePath,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hidden: Option<bool>,
    },
    #[serde(rename = "git")]
    Git {
        repository: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hidden: Option<bool>,
    },
}

/// `Reference.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceInfo {
    pub name: String,
    pub path: AbsolutePath,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    pub source: ReferenceSource,
}
