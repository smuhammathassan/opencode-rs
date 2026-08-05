//! From reference/packages/schema/src/reference.ts

use crate::define_event;
use crate::schema::AbsolutePath;
use serde::{Deserialize, Serialize};

define_event! {
    /// `reference.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "reference.updated",
        data: crate::schema::Empty,
    }
}

/// `Reference.Source` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Source {
    Local(LocalSource),
    Git(GitSource),
}

/// `Reference.LocalSource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocalSource {
    #[serde(rename = "type")]
    pub r#type: LocalSourceType,
    pub path: AbsolutePath,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hidden: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum LocalSourceType {
    #[serde(rename = "local")]
    Value,
}

/// `Reference.GitSource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GitSource {
    #[serde(rename = "type")]
    pub r#type: GitSourceType,
    pub repository: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hidden: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum GitSourceType {
    #[serde(rename = "git")]
    Value,
}

/// `Reference.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub name: String,
    pub path: AbsolutePath,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hidden: Option<bool>,
    pub source: Source,
}

/// `Reference.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Updated;
    pub use crate::event::Definition;

    /// `Reference.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "reference.updated",
        durable: None,
    }];
}

/// `Reference.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
