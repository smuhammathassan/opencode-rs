//! From reference/packages/schema/src/project.ts

use crate::define_event;
use crate::project_id::ProjectID;
use crate::schema::NonNegativeInt;
use serde::{Deserialize, Serialize};

/// `Project.ID`.
pub type ID = ProjectID;

/// `Project.Vcs`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Vcs {
    #[serde(rename = "git")]
    Git,
}

/// `Project.Icon`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Icon {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(rename = "override", skip_serializing_if = "Option::is_none", default)]
    pub override_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
}

/// `Project.Commands`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Commands {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start: Option<String>,
}

/// `Project.Time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Time {
    pub created: NonNegativeInt,
    pub updated: NonNegativeInt,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub initialized: Option<NonNegativeInt>,
}

/// `Project` (`Project.Info`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    pub worktree: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub vcs: Option<Vcs>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub icon: Option<Icon>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commands: Option<Commands>,
    pub time: Time,
    pub sandboxes: Vec<String>,
}

define_event! {
    /// `project.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "project.updated",
        data: Info,
    }
}

/// `Project.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Updated;
    pub use crate::event::Definition;

    /// `Project.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "project.updated",
        durable: None,
    }];
}

/// `Project.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
