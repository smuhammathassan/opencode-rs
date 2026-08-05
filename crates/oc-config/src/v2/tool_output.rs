// From reference/packages/core/src/config/tool-output.ts

use crate::jsnum::PositiveInt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<PositiveInt>,
}
