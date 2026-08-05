// From reference/packages/core/src/config/reference.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Git {
    pub repository: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Local {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

/// `Entry` = `Union([String, Git, Local])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Url(String),
    Git(Git),
    Local(Local),
}

impl Entry {
    pub fn description(&self) -> Option<&str> {
        match self {
            Entry::Url(_) => None,
            Entry::Git(git) => git.description.as_deref(),
            Entry::Local(local) => local.description.as_deref(),
        }
    }

    pub fn hidden(&self) -> Option<bool> {
        match self {
            Entry::Url(_) => None,
            Entry::Git(git) => git.hidden,
            Entry::Local(local) => local.hidden,
        }
    }

    /// A reference resolves to a local path when it is a bare path-like string
    /// or a `Local` entry (see `local()` in the config-reference plugin).
    pub fn is_local(&self) -> bool {
        match self {
            Entry::Url(url) => url.starts_with('.') || url.starts_with('/') || url.starts_with('~'),
            Entry::Local(_) => true,
            Entry::Git(_) => false,
        }
    }

    pub fn is_git(&self) -> bool {
        matches!(self, Entry::Git(_))
    }

    pub fn is_url(&self) -> bool {
        matches!(self, Entry::Url(_))
    }
}

/// `Info` = `Record(Schema.String, Entry)`.
pub type Info = IndexMap<String, Entry>;
