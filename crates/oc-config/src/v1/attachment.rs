// From reference/packages/core/src/v1/config/attachment.ts

use crate::jsnum::PositiveInt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Image {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_resize: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_width: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_height: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_base64_bytes: Option<PositiveInt>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<Image>,
}
