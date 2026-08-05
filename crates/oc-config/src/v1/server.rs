// From reference/packages/core/src/v1/config/server.ts

use crate::jsnum::PositiveInt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Server {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mdns: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mdnsDomain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cors: Option<Vec<String>>,
}
