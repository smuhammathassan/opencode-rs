// From reference/packages/core/src/v1/config/plugin.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `Schema.Record(Schema.String, Schema.Unknown)` — plugin options.
pub type Options = IndexMap<String, Value>;

/// `Spec` = `Union([String, [String, Options], { package, options? }])`.
///
/// The tuple form is the v1 configuration shape. The object form is accepted
/// as the v2-compatible spelling used by newer OpenCode configurations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Spec {
    Package(String),
    Entry((String, Options)),
    Object {
        package: String,
        #[serde(default, skip_serializing_if = "Options::is_empty")]
        options: Options,
    },
}

impl Spec {
    pub fn package(&self) -> &str {
        match self {
            Spec::Package(package) => package,
            Spec::Entry((package, _)) => package,
            Spec::Object { package, .. } => package,
        }
    }

    pub fn options(&self) -> Option<&Options> {
        match self {
            Spec::Package(_) => None,
            Spec::Entry((_, options)) => Some(options),
            Spec::Object { options, .. } if options.is_empty() => None,
            Spec::Object { options, .. } => Some(options),
        }
    }
}
