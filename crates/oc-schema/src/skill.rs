//! From reference/packages/schema/src/skill.ts

use crate::schema::AbsolutePath;
use serde::{Deserialize, Serialize};

/// `SkillV2.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub slash: Option<bool>,
    pub location: AbsolutePath,
    pub content: String,
}

/// `SkillV2.DirectorySource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DirectorySource {
    #[serde(rename = "type")]
    pub r#type: DirectorySourceType,
    pub path: AbsolutePath,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum DirectorySourceType {
    #[serde(rename = "directory")]
    Value,
}

/// `SkillV2.UrlSource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UrlSource {
    #[serde(rename = "type")]
    pub r#type: UrlSourceType,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum UrlSourceType {
    #[serde(rename = "url")]
    Value,
}

/// `SkillV2.EmbeddedSource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EmbeddedSource {
    #[serde(rename = "type")]
    pub r#type: EmbeddedSourceType,
    pub skill: Info,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum EmbeddedSourceType {
    #[serde(rename = "embedded")]
    Value,
}

/// `SkillV2.Source` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Source {
    Directory(DirectorySource),
    Url(UrlSource),
    Embedded(EmbeddedSource),
}

impl Source {
    /// `Source.equals(a, b)` from skill.ts.
    pub fn equals(a: &Source, b: &Source) -> bool {
        match (a, b) {
            (Source::Directory(x), Source::Directory(y)) => x.path == y.path,
            (Source::Url(x), Source::Url(y)) => x.url == y.url,
            (Source::Embedded(x), Source::Embedded(y)) => x.skill.name == y.skill.name,
            _ => false,
        }
    }

    /// `Source.key(source)` from skill.ts.
    pub fn key(&self) -> String {
        match self {
            Source::Directory(s) => format!("directory:{}", s.path),
            Source::Url(s) => format!("url:{}", s.url),
            Source::Embedded(s) => format!("embedded:{}", s.skill.name),
        }
    }
}
