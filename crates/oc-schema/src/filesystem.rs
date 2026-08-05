//! From reference/packages/schema/src/filesystem.ts

use crate::define_event;
use crate::schema::{NonNegativeInt, PositiveInt, RelativePath};
use serde::{Deserialize, Serialize};

define_event! {
    /// `file.edited`.
    pub struct Edited {
        tag: EditedTag,
        r#type: "file.edited",
        data: EditedData,
    }
}

/// Payload of `file.edited`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct EditedData {
    pub file: String,
}

/// `FileSystem.Entry.type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum EntryType {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "directory")]
    Directory,
}

/// `FileSystem.Entry`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: RelativePath,
    #[serde(rename = "type")]
    pub r#type: EntryType,
}

/// `FileSystem.Submatch`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Submatch {
    pub text: String,
    pub start: NonNegativeInt,
    pub end: NonNegativeInt,
}

/// `FileSystem.Match`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Match {
    pub entry: Entry,
    pub line: PositiveInt,
    pub offset: NonNegativeInt,
    pub text: String,
    pub submatches: Vec<Submatch>,
}

/// `FileSystem.FindInput.type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FindType {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "directory")]
    Directory,
}

/// `FileSystem.FindInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FindInput {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub r#type: Option<FindType>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<PositiveInt>,
}

/// `FileSystem.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::Edited;
    pub use crate::event::Definition;

    /// `FileSystem.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[Definition {
        r#type: "file.edited",
        durable: None,
    }];
}

/// `FileSystem.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
