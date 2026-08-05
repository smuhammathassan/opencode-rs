//! Filesystem types.
//! From reference/packages/schema/src/filesystem.ts.

use crate::types::location::LocationQueryRef;
use crate::types::schema::RelativePath;

/// `FileSystem.Entry`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemEntry {
    pub path: RelativePath,
    #[serde(rename = "type")]
    pub kind: FileSystemEntryType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileSystemEntryType {
    File,
    Directory,
}

/// `FilesListInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilesListInput {
    pub location: Option<LocationQueryRef>,
    pub path: Option<RelativePath>,
}

/// `FilesFindInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilesFindInput {
    pub location: Option<LocationQueryRef>,
    pub query: String,
    pub kind: Option<FileSystemEntryType>,
    pub limit: Option<u64>,
}
