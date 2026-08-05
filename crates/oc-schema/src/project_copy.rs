//! From reference/packages/schema/src/project-copy.ts

use crate::project_id::ProjectID;
use crate::schema::AbsolutePath;
use serde::{Deserialize, Serialize};

/// `ProjectCopy.StrategyID`.
pub type StrategyID = String;

/// `ProjectCopy.CreateInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CreateInput {
    #[serde(rename = "projectID")]
    pub project_id: ProjectID,
    pub strategy: StrategyID,
    #[serde(rename = "sourceDirectory")]
    pub source_directory: AbsolutePath,
    pub directory: AbsolutePath,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
}

/// `ProjectCopy.RemoveInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RemoveInput {
    #[serde(rename = "projectID")]
    pub project_id: ProjectID,
    pub directory: AbsolutePath,
    pub force: bool,
}

/// `ProjectCopy.Copy`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Copy {
    pub directory: AbsolutePath,
}
