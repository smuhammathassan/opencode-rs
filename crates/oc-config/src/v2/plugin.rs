// From reference/packages/core/src/config/plugin.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub package: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<IndexMap<String, Value>>,
}

/// `Plugin` = `Union([String, Entry])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Plugin {
    Package(String),
    Entry(Entry),
}
/// `Plugins` = `Array<Plugin>`.
pub type Plugins = Vec<Plugin>;
