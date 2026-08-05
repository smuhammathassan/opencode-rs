// From reference/packages/core/src/v1/config/layout.ts

use serde::{Deserialize, Serialize};

/// `Schema.Literals(["auto", "stretch"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Auto,
    Stretch,
}
