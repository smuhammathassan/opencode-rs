//! From reference/packages/schema/src/command.ts

use crate::model;
use serde::{Deserialize, Serialize};

/// `CommandV2.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub name: String,
    pub template: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<model::Ref>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subtask: Option<bool>,
}
