//! Branded id types and shared literal types.
//! From reference/packages/llm/src/schema/ids.ts

use serde::{Deserialize, Serialize};

/// `ProviderMetadata` — `Record<string, Record<string, unknown>>`.
/// From reference/packages/schema/src/llm.ts (`ProviderMetadata`)
pub type ProviderMetadata = std::collections::BTreeMap<String, serde_json::Map<String, serde_json::Value>>;

/// Stable string identifier for a protocol implementation.
/// From reference/packages/llm/src/schema/ids.ts (`ProtocolID`)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolId(pub String);

/// Stable string identifier for the runnable route.
/// From reference/packages/llm/src/schema/ids.ts (`RouteID`)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RouteId(pub String);

/// Branded model id (`LLM.ModelID`).
/// From reference/packages/llm/src/schema/ids.ts (`ModelID`)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub String);

impl ModelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Branded provider id (`LLM.ProviderID`).
/// From reference/packages/llm/src/schema/ids.ts (`ProviderID`)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ProviderId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for ProviderId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Reasoning effort levels.
/// From reference/packages/llm/src/schema/ids.ts (`ReasoningEfforts`)
pub const REASONING_EFFORTS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Text verbosity.
/// From reference/packages/llm/src/schema/ids.ts (`TextVerbosity`)
pub const TEXT_VERBOSITY: [&str; 3] = ["low", "medium", "high"];

/// Message roles.
/// From reference/packages/llm/src/schema/ids.ts (`MessageRole`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::System => f.write_str("system"),
            MessageRole::User => f.write_str("user"),
            MessageRole::Assistant => f.write_str("assistant"),
            MessageRole::Tool => f.write_str("tool"),
        }
    }
}

/// Finish reasons.
/// From reference/packages/llm/src/schema/ids.ts (`FinishReason`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error,
    Unknown,
}

impl FinishReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            FinishReason::Stop => "stop",
            FinishReason::Length => "length",
            FinishReason::ToolCalls => "tool-calls",
            FinishReason::ContentFilter => "content-filter",
            FinishReason::Error => "error",
            FinishReason::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// JSON schema — `Record<string, unknown>`.
/// From reference/packages/llm/src/schema/ids.ts (`JsonSchema`)
pub type JsonSchema = serde_json::Value;

/// `CacheHint` — prompt caching marker on a part.
/// From reference/packages/llm/src/schema/options.ts (`CacheHint`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheHintType {
    Ephemeral,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheHint {
    #[serde(rename = "type")]
    pub kind: CacheHintType,
    #[serde(rename = "ttlSeconds", skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
}
