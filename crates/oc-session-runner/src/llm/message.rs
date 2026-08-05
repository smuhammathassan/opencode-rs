use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::{ProviderMetadata, ToolResultValue};

/// `LLM.SystemPart`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemPart {
    #[serde(rename = "type")]
    pub kind: SystemPartKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemPartKind {
    Text,
}

impl SystemPart {
    pub fn make(text: impl Into<String>) -> Self {
        Self {
            kind: SystemPartKind::Text,
            text: text.into(),
            cache: None,
            metadata: None,
        }
    }

    /// Normalize optional string/system-part/array input into a vector.
    /// From reference/packages/llm/src/schema/messages.ts (`SystemPart.content`)
    pub fn content(input: Option<&[SystemPart]>) -> Vec<SystemPart> {
        input.map(|parts| parts.to_vec()).unwrap_or_default()
    }
}

/// `LLM.Content.Text`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    #[serde(rename = "type")]
    pub kind: TextPartKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextPartKind {
    Text,
}

impl TextPart {
    pub fn make(text: impl Into<String>) -> Self {
        Self {
            kind: TextPartKind::Text,
            text: text.into(),
            cache: None,
            metadata: None,
            provider_metadata: None,
        }
    }
}

/// `LLM.Content.Media`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPart {
    #[serde(rename = "type")]
    pub kind: MediaPartKind,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaPartKind {
    Media,
}

/// `LLM.Content.Reasoning`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningPart {
    #[serde(rename = "type")]
    pub kind: ReasoningPartKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningPartKind {
    Reasoning,
}

/// `LLM.Content.ToolCall`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallPart {
    #[serde(rename = "type")]
    pub kind: ToolCallPartKind,
    pub id: String,
    pub name: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolCallPartKind {
    #[serde(rename = "tool-call")]
    ToolCall,
}

/// `LLM.Content.ToolResult`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultPart {
    #[serde(rename = "type")]
    pub kind: ToolResultPartKind,
    pub id: String,
    pub name: String,
    pub result: ToolResultValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolResultPartKind {
    #[serde(rename = "tool-result")]
    ToolResult,
}

/// `LLM.ContentPart` tagged union.
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ContentPart {
    Text(TextPart),
    Media(MediaPart),
    #[serde(rename = "tool-call")]
    ToolCall(ToolCallPart),
    #[serde(rename = "tool-result")]
    ToolResult(ToolResultPart),
    Reasoning(ReasoningPart),
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextPart::make(text))
    }
}

/// `LLM.MessageRole`
/// /// From reference/packages/llm/src/schema/ids.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// `LLM.Message`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub role: MessageRole,
    pub content: Vec<ContentPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<serde_json::Map<String, Value>>,
}

impl Message {
    pub fn user(content: Vec<ContentPart>) -> Self {
        Self {
            id: None,
            role: MessageRole::User,
            content,
            metadata: None,
            native: None,
        }
    }

    pub fn assistant(content: Vec<ContentPart>) -> Self {
        Self {
            id: None,
            role: MessageRole::Assistant,
            content,
            metadata: None,
            native: None,
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            id: None,
            role: MessageRole::System,
            content: vec![ContentPart::text(text)],
            metadata: None,
            native: None,
        }
    }

    pub fn tool(part: ToolResultPart) -> Self {
        Self {
            id: None,
            role: MessageRole::Tool,
            content: vec![ContentPart::ToolResult(part)],
            metadata: None,
            native: None,
        }
    }
}

/// `LLM.ToolDefinition`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<serde_json::Map<String, Value>>,
}

/// `LLM.ToolChoice`
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolChoice {
    #[serde(rename = "type")]
    pub kind: ToolChoiceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolChoiceKind {
    Auto,
    None,
    Required,
    Tool,
}

impl ToolChoice {
    pub fn mode(input: &str) -> Self {
        match input {
            "auto" => Self {
                kind: ToolChoiceKind::Auto,
                name: None,
            },
            "none" => Self {
                kind: ToolChoiceKind::None,
                name: None,
            },
            "required" => Self {
                kind: ToolChoiceKind::Required,
                name: None,
            },
            name => Self {
                kind: ToolChoiceKind::Tool,
                name: Some(name.to_string()),
            },
        }
    }
}

/// `LLM.Model` — the minimal subset the runner inspects (id + provider for
/// provider-metadata reuse and equality checks).
/// /// From reference/packages/llm/src/schema/options.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub provider: String,
}

impl Model {
    pub fn make(id: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
        }
    }
}

/// `LLM.Request` — the canonical request passed to `LLMClient.stream`.
/// /// From reference/packages/llm/src/schema/messages.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LLMRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub model: Model,
    #[serde(default)]
    pub system: Vec<SystemPart>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<GenerationOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpOptions>,
}

impl LLMRequest {
    pub fn system_content(&self) -> Vec<SystemPart> {
        SystemPart::content(Some(&self.system))
    }
}

/// `LLM.GenerationOptions`
/// /// From reference/packages/llm/src/schema/options.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

/// `LLM.ProviderOptions`
/// /// From reference/packages/llm/src/schema/options.ts
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<OpenAIOptions>,
}

/// OpenAI-specific provider options (`promptCacheKey`).
/// /// From reference/packages/llm/src/schema/options.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
}

/// `LLM.HttpOptions`
/// /// From reference/packages/llm/src/schema/options.ts
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Map<String, Value>>,
}
