// From reference/packages/core/src/v1/config/agent.ts

use super::permission::{Action, Info as PermissionInfo, Rule};
use crate::jsnum::{de_f64_opt, serialize_js_number_opt, PositiveInt};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// `mode` = `Schema.Literals(["subagent", "primary", "all"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Subagent,
    Primary,
    All,
}

/// Known keys that are real schema fields (kept out of `options` during
/// normalization). Note `name` is *not* here — it is not a schema field, so it
/// survives as a top-level rest key without being copied into `options`.
const KNOWN_KEYS: [&str; 15] = [
    "model",
    "variant",
    "prompt",
    "description",
    "temperature",
    "top_p",
    "mode",
    "hidden",
    "color",
    "steps",
    "maxSteps",
    "options",
    "permission",
    "disable",
    "tools",
];

/// Raw decoded shape before normalization.
#[derive(Deserialize)]
struct Raw {
    model: Option<String>,
    variant: Option<String>,
    #[serde(default, deserialize_with = "de_f64_opt")]
    temperature: Option<f64>,
    #[serde(default, deserialize_with = "de_f64_opt")]
    top_p: Option<f64>,
    prompt: Option<String>,
    tools: Option<IndexMap<String, bool>>,
    disable: Option<bool>,
    description: Option<String>,
    mode: Option<Mode>,
    hidden: Option<bool>,
    options: Option<IndexMap<String, Value>>,
    #[serde(default, deserialize_with = "de_color")]
    color: Option<String>,
    steps: Option<PositiveInt>,
    #[serde(rename = "maxSteps")]
    max_steps: Option<PositiveInt>,
    permission: Option<PermissionInfo>,
    #[serde(flatten)]
    rest: IndexMap<String, Value>,
}

fn de_color<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if let Some(color) = &value {
        if !is_valid_color(color) {
            return Err(serde::de::Error::custom(format!(
                "Invalid color \"{color}\". Expected a hex color like \"#FF5733\" or a theme color like \"primary\"."
            )));
        }
    }
    Ok(value)
}

/// `Color` = hex `#rrggbb` or one of the named theme colors.
pub fn is_valid_color(color: &str) -> bool {
    matches!(
        color,
        "primary" | "secondary" | "accent" | "success" | "warning" | "error" | "info"
    ) || (color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit()))
}

/// `Info` — an agent config with zod `decodeTo` normalization applied:
/// unknown keys are copied into `options`, `tools` is translated to
/// `permission`, and `steps` falls back to the deprecated `maxSteps`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Info {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_js_number_opt"
    )]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "serialize_js_number_opt")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<IndexMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    pub options: IndexMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxSteps")]
    pub max_steps: Option<PositiveInt>,
    pub permission: PermissionInfo,
    #[serde(flatten)]
    pub rest: IndexMap<String, Value>,
}

impl Info {
    /// Builds an agent straight from markdown frontmatter plus a `prompt`,
    /// mirroring `ConfigAgent.load` / `ConfigAgent.loadMode`.
    pub fn from_parts(
        name: String,
        data: IndexMap<String, Value>,
        prompt: String,
    ) -> Result<Info, serde_json::Error> {
        let mut map = data;
        map.insert("name".to_string(), Value::String(name));
        map.insert("prompt".to_string(), Value::String(prompt));
        serde_json::from_value(Value::Object(map.into_iter().collect()))
    }
}

impl<'de> Deserialize<'de> for Info {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Raw::deserialize(deserializer)?;
        normalize(raw).map_err(serde::de::Error::custom)
    }
}

fn normalize(raw: Raw) -> Result<Info, String> {
    let mut options: IndexMap<String, Value> = raw.options.clone().unwrap_or_default();
    for (key, value) in &raw.rest {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            options.insert(key.clone(), value.clone());
        }
    }

    let mut permission = PermissionInfo::default();
    if let Some(tools) = &raw.tools {
        for (tool, enabled) in tools {
            let action = if *enabled { Action::Allow } else { Action::Deny };
            let key = if tool == "write" || tool == "edit" || tool == "patch" {
                "edit".to_string()
            } else {
                tool.clone()
            };
            permission.insert(key, Rule::Action(action));
        }
    }
    if let Some(explicit) = &raw.permission {
        permission.assign(explicit);
    }

    let steps = raw.steps.or(raw.max_steps);

    Ok(Info {
        model: raw.model,
        variant: raw.variant,
        temperature: raw.temperature,
        top_p: raw.top_p,
        prompt: raw.prompt,
        tools: raw.tools,
        disable: raw.disable,
        description: raw.description,
        mode: raw.mode,
        hidden: raw.hidden,
        options,
        color: raw.color,
        steps,
        max_steps: raw.max_steps,
        permission,
        rest: raw.rest,
    })
}
