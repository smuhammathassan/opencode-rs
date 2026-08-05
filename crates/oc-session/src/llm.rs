/// Local mirror of `reference/packages/llm/src/schema/events.ts` and
/// `reference/packages/llm/src/schema/messages.ts` (subset) — the provider
/// neutral event stream the session processor consumes.
///
/// TODO(integration): promote to oc-llm once that crate ships its schema.
use serde::{Deserialize, Serialize};

use crate::JsonMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_cached_input_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LLMEvent {
    #[serde(rename = "step-start")]
    StepStart {
        index: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "text-start")]
    TextStart {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "text-delta")]
    TextDelta {
        id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "text-end")]
    TextEnd {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "reasoning-start")]
    ReasoningStart {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "reasoning-delta")]
    ReasoningDelta {
        id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "reasoning-end")]
    ReasoningEnd {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "tool-input-start")]
    ToolInputStart {
        id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "tool-input-delta")]
    ToolInputDelta {
        id: String,
        name: String,
        text: String,
    },
    #[serde(rename = "tool-input-end")]
    ToolInputEnd {
        id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "tool-call")]
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "tool-result")]
    ToolResult {
        id: String,
        name: String,
        result: ToolResultValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "tool-error")]
    ToolError {
        id: String,
        name: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "step-finish")]
    StepFinish {
        index: f64,
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "finish")]
    Finish {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "provider-error")]
    ProviderError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        retryable: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultValue {
    Text(String),
    Json(serde_json::Value),
    Error(JsonMap),
}

impl LLMEvent {
    pub fn text(&self) -> Option<&str> {
        match self {
            LLMEvent::TextDelta { text, .. } => Some(text),
            _ => None,
        }
    }
}

/// ModelMessage shapes used by `MessageV2::to_model_messages` and the
/// compaction prompt builder. Mirrors the AI SDK `ModelMessage` / content
/// part union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text(TextPart),
    Media(MediaPart),
    Reasoning(ReasoningContent),
    ToolCall(ToolCallPart),
    ToolResult(ToolResultPart),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningContent {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub result: ToolResultValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMessage {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub parts: Vec<MessagePart>,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessagePart {
    Text(UiTextPart),
    File(UiFilePart),
    StepStart(StepStartContent),
    Reasoning(UiReasoningPart),
    Tool(JsonMap),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTextPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiFilePart {
    #[serde(rename = "type")]
    pub type_: String,
    pub url: String,
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartContent {
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiReasoningPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
}
