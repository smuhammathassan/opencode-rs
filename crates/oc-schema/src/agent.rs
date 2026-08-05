//! From reference/packages/schema/src/agent.ts

use crate::model;
use crate::permission::Ruleset;
use crate::provider;
use crate::schema::PositiveInt;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// `AgentV2.ID`.
pub type ID = String;

/// `Agent.Color` — a `#RRGGBB` hex string or one of the named literal colors.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub String);

pub const COLOR_PRIMARY: &str = "primary";
pub const COLOR_SECONDARY: &str = "secondary";
pub const COLOR_ACCENT: &str = "accent";
pub const COLOR_SUCCESS: &str = "success";
pub const COLOR_WARNING: &str = "warning";
pub const COLOR_ERROR: &str = "error";
pub const COLOR_INFO: &str = "info";

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if is_color(&value) {
            Ok(Color(value))
        } else {
            Err(serde::de::Error::custom(format!(
                "invalid color: expected #RRGGBB or a named color, got {value}"
            )))
        }
    }
}

fn is_color(value: &str) -> bool {
    if matches!(
        value,
        "primary" | "secondary" | "accent" | "success" | "warning" | "error" | "info"
    ) {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

/// `Agent.Info.mode`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    #[serde(rename = "subagent")]
    Subagent,
    #[serde(rename = "primary")]
    Primary,
    #[serde(rename = "all")]
    All,
}

/// `AgentV2.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<model::Ref>,
    pub request: provider::Request,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    pub mode: Mode,
    pub hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub steps: Option<PositiveInt>,
    pub permissions: Ruleset,
}

/// `Agent.empty(id)`.
pub fn empty(id: ID) -> Info {
    Info {
        id,
        model: None,
        request: provider::Request {
            headers: Default::default(),
            body: Default::default(),
        },
        system: None,
        description: None,
        mode: Mode::All,
        hidden: false,
        color: None,
        steps: None,
        permissions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_accepts_hex_and_literals() {
        assert!(is_color("#aBc123"));
        assert!(is_color("primary"));
        assert!(!is_color("#12345"));
        assert!(!is_color("not-a-color"));
    }
}
