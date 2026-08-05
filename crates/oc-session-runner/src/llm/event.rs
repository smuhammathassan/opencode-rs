use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::ProviderMetadata;

/// `Tool.TextContent`
/// /// From reference/packages/schema/src/llm.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "file")]
    File {
        uri: String,
        mime: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

impl ToolContent {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            Self::File { .. } => None,
        }
    }
}

/// `LLM.ToolResult` tagged value.
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolResultValue {
    Json { value: Value },
    Text { value: Value },
    Error { value: Value },
    Content { value: Vec<ToolContent> },
}

impl ToolResultValue {
    /// Wrap a raw value, reusing the value when it is already a tool-result.
    /// /// From reference/packages/llm/src/schema/messages.ts (`ToolResultValue.make`)
    pub fn make(value: Value, kind: ToolResultKind) -> Self {
        if let Ok(existing) = serde_json::from_value::<ToolResultValue>(value.clone()) {
            return existing;
        }
        match kind {
            ToolResultKind::Json => Self::Json { value },
            ToolResultKind::Text => Self::Text { value },
            ToolResultKind::Error => Self::Error { value },
            ToolResultKind::Content => {
                let items = value
                    .as_array()
                    .cloned()
                    .and_then(|items| {
                        serde_json::from_value::<Vec<ToolContent>>(Value::Array(items)).ok()
                    })
                    .unwrap_or_default();
                Self::Content { value: items }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolResultKind {
    Json,
    Text,
    Error,
    Content,
}

impl ToolResultValue {
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// `LLM.ToolOutput`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub structured: Value,
    #[serde(default)]
    pub content: Vec<ToolContent>,
}

impl ToolOutput {
    pub fn make(structured: Value, content: Vec<ToolContent>) -> Self {
        Self {
            structured,
            content,
        }
    }

    pub fn from_result_value(result: &ToolResultValue) -> Option<Self> {
        match result {
            ToolResultValue::Json { value } => Some(Self {
                structured: value.clone(),
                content: Vec::new(),
            }),
            ToolResultValue::Text { value } => Some(Self {
                structured: Value::Object(Default::default()),
                content: vec![ToolContent::text(tool_result_text(value))],
            }),
            ToolResultValue::Content { value } => Some(Self {
                structured: Value::Object(Default::default()),
                content: value.clone(),
            }),
            ToolResultValue::Error { .. } => None,
        }
    }

    /// `ToolOutput.toResultValue` — lowers to the model-facing result value.
    /// /// From reference/packages/llm/src/schema/messages.ts
    pub fn to_result_value(&self) -> ToolResultValue {
        if self.content.is_empty() {
            return ToolResultValue::Json {
                value: self.structured.clone(),
            };
        }
        if self.content.len() == 1 {
            if let Some(text) = self.content[0].as_text() {
                return ToolResultValue::Text {
                    value: Value::String(text.to_string()),
                };
            }
        }
        ToolResultValue::Content {
            value: self.content.clone(),
        }
    }
}

fn tool_result_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

/// `LLM.Usage`
/// /// From reference/packages/llm/src/schema/events.ts
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    pub provider_metadata: Option<ProviderMetadata>,
}

impl Usage {
    /// Visible output tokens — `outputTokens` minus `reasoningTokens`, clamped.
    /// /// From reference/packages/llm/src/schema/events.ts
    pub fn visible_output_tokens(&self) -> f64 {
        let output = self.output_tokens.unwrap_or(0.0);
        let reasoning = self.reasoning_tokens.unwrap_or(0.0);
        f64::max(0.0, output - reasoning)
    }
}

/// Provider error classification.
/// /// From reference/packages/llm/src/schema/errors.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderFailureClassification {
    #[serde(rename = "context-overflow")]
    ContextOverflow,
}

/// `LLM.Event.ProviderError`
/// /// From reference/packages/llm/src/schema/events.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderErrorEvent {
    #[serde(rename = "type")]
    pub kind: ProviderErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<ProviderFailureClassification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderErrorKind {
    #[serde(rename = "provider-error")]
    ProviderError,
}

/// `LLM.Event` tagged union — the shared stream event surface.
/// /// From reference/packages/llm/src/schema/events.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LLMEvent {
    #[serde(rename = "step-start")]
    StepStart { index: f64 },
    #[serde(rename = "text-start")]
    TextStart {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "text-delta")]
    TextDelta {
        id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "text-end")]
    TextEnd {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-start")]
    ReasoningStart {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-delta")]
    ReasoningDelta {
        id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning-end")]
    ReasoningEnd {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-input-start")]
    ToolInputStart {
        id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
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
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-call")]
    ToolCall {
        id: String,
        name: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-result")]
    ToolResult {
        id: String,
        name: String,
        result: ToolResultValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<ToolOutput>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-error")]
    ToolError {
        id: String,
        name: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "step-finish")]
    StepFinish {
        index: f64,
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "finish")]
    Finish {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "provider-error")]
    ProviderError(ProviderErrorEvent),
}

impl LLMEvent {
    pub fn provider_metadata(&self) -> Option<&ProviderMetadata> {
        match self {
            Self::TextStart {
                provider_metadata, ..
            }
            | Self::TextDelta {
                provider_metadata, ..
            }
            | Self::TextEnd {
                provider_metadata, ..
            }
            | Self::ReasoningStart {
                provider_metadata, ..
            }
            | Self::ReasoningDelta {
                provider_metadata, ..
            }
            | Self::ReasoningEnd {
                provider_metadata, ..
            }
            | Self::ToolInputStart {
                provider_metadata, ..
            }
            | Self::ToolInputEnd {
                provider_metadata, ..
            }
            | Self::ToolCall {
                provider_metadata, ..
            }
            | Self::ToolResult {
                provider_metadata, ..
            }
            | Self::ToolError {
                provider_metadata, ..
            }
            | Self::StepFinish {
                provider_metadata, ..
            }
            | Self::Finish {
                provider_metadata, ..
            } => provider_metadata.as_ref(),
            Self::ProviderError(event) => event.provider_metadata.as_ref(),
            Self::StepStart { .. } | Self::ToolInputDelta { .. } => None,
        }
    }
}
