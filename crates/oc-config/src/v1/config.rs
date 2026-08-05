// From reference/packages/core/src/v1/config/config.ts

use super::agent;
use super::attachment;
use super::command;
use super::formatter;
use super::layout;
use super::lsp;
use super::mcp;
use super::permission;
use super::plugin;
use super::provider;
use super::server;
use crate::jsnum::{NonNegativeInt, PositiveInt};
use crate::v2::experimental::Policy;
use crate::v2::reference::Info as ReferenceInfo;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// `logLevel` = `Schema.Literals(["DEBUG", "INFO", "WARN", "ERROR"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    #[serde(rename = "DEBUG")]
    Debug,
    #[serde(rename = "INFO")]
    Info,
    #[serde(rename = "WARN")]
    Warn,
    #[serde(rename = "ERROR")]
    Error,
}

/// `share` = `Schema.Literals(["manual", "auto", "disabled"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Share {
    Manual,
    Auto,
    Disabled,
}

/// `autoupdate` = `Schema.Union([Schema.Boolean, Schema.Literal("notify")])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoUpdate {
    Enabled(bool),
    Notify,
}

impl Serialize for AutoUpdate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            AutoUpdate::Enabled(value) => serializer.serialize_bool(*value),
            AutoUpdate::Notify => serializer.serialize_str("notify"),
        }
    }
}

impl<'de> Deserialize<'de> for AutoUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Text(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Bool(value) => Ok(AutoUpdate::Enabled(value)),
            Raw::Text(value) if value == "notify" => Ok(AutoUpdate::Notify),
            Raw::Text(value) => Err(serde::de::Error::custom(format!(
                "Expected true, false, or \"notify\" but got \"{value}\""
            ))),
        }
    }
}

/// `watcher` — `{ ignore?: string[] }`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Watcher {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,
}

/// `skills` — `{ paths?: string[], urls?: string[] }`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Skills {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urls: Option<Vec<String>>,
}

/// `enterprise` — `{ url?: string }`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Enterprise {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// `tool_output` — `{ max_lines?, max_bytes? }` (positive ints).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<PositiveInt>,
}

/// `compaction` — auto/prune toggles plus token budget fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Compaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prune: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_turns: Option<NonNegativeInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserve_recent_tokens: Option<NonNegativeInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved: Option<NonNegativeInt>,
}

/// `experimental` — feature toggles and policy statements.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Experimental {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_paste_summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_tool: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openTelemetry: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continue_loop_on_deny: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_timeout: Option<PositiveInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policies: Option<Vec<Policy>>,
}

/// The parsed `opencode.json`/`opencode.jsonc` document. Field names and
/// optionality mirror `ConfigV1.Info` verbatim; absent keys are omitted on
/// serialization.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<LogLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<server::Server>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<IndexMap<String, command::Info>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Skills>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<ReferenceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<ReferenceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watcher: Option<Watcher>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<Vec<plugin::Spec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<Share>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoshare: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoupdate: Option<AutoUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_providers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_providers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_depth: Option<NonNegativeInt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<IndexMap<String, agent::Info>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<IndexMap<String, agent::Info>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<IndexMap<String, provider::Info>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<IndexMap<String, mcp::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formatter: Option<formatter::Info>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsp: Option<lsp::Info>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<layout::Layout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<permission::Info>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<IndexMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<attachment::Info>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise: Option<Enterprise>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<ToolOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<Compaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Experimental>,
}

impl Info {
    /// Providers merged over from legacy `config.toml` (see `loadGlobal`).
    pub fn provider_mut(&mut self) -> &mut IndexMap<String, provider::Info> {
        self.provider.get_or_insert_with(IndexMap::new)
    }

    pub fn agents(&self) -> &IndexMap<String, agent::Info> {
        static EMPTY: std::sync::LazyLock<IndexMap<String, agent::Info>> =
            std::sync::LazyLock::new(IndexMap::new);
        self.agent.as_ref().unwrap_or(&EMPTY)
    }

    pub fn commands(&self) -> &IndexMap<String, command::Info> {
        static EMPTY: std::sync::LazyLock<IndexMap<String, command::Info>> =
            std::sync::LazyLock::new(IndexMap::new);
        self.command.as_ref().unwrap_or(&EMPTY)
    }
}
