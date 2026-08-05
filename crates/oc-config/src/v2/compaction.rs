// From reference/packages/core/src/config/compaction.ts

use crate::jsnum::NonNegativeInt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Keep {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<NonNegativeInt>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prune: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep: Option<Keep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer: Option<NonNegativeInt>,
}
