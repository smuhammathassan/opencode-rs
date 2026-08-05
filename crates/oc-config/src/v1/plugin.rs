// From reference/packages/core/src/v1/config/plugin.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `Schema.Record(Schema.String, Schema.Unknown)` — plugin options.
pub type Options = IndexMap<String, Value>;

/// `Spec` = `Union([String, [String, Options]])`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Spec {
    Package(String),
    Entry((String, Options)),
}

impl Spec {
    pub fn package(&self) -> &str {
        match self {
            Spec::Package(package) => package,
            Spec::Entry((package, _)) => package,
        }
    }

    pub fn options(&self) -> Option<&Options> {
        match self {
            Spec::Package(_) => None,
            Spec::Entry((_, options)) => Some(options),
        }
    }
}
