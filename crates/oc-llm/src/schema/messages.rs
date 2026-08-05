//! Content parts, messages, tool definitions and the canonical request.
//! From reference/packages/llm/src/schema/messages.ts

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::{CacheHint, MessageRole, ProviderMetadata};
use super::options::{CachePolicy, GenerationOptions, HttpOptions, Model};
use super::ids::JsonSchema;

/// `SystemPart` — `{ type: "text", text, cache?, metadata? }`.
/// From reference/packages/llm/src/schema/messages.ts (`SystemPart`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemPart {
    #[serde(rename = "type", default = "system_part_default_type")]
    pub part_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

fn system_part_default_type() -> String {
    "text".to_string()
}

impl SystemPart {
    /// `SystemPart.make(text)` — `{ type: "text", text }`.
    pub fn make(text: impl Into<String>) -> Self {
        Self { part_type: "text".to_string(), text: text.into(), cache: None, metadata: None }
    }

    /// `SystemPart.content(input)` — normalize into a `Vec<SystemPart>`.
    pub fn content(input: Option<&SystemPartRef>) -> Vec<SystemPart> {
        match input {
            None => vec![],
            Some(SystemPartRef::One(part)) => vec![part.clone()],
            Some(SystemPartRef::Many(parts)) => parts.clone(),
            Some(SystemPartRef::String(text)) => vec![SystemPart::make(text.clone())],
        }
    }
}

/// Ergonomic system-part input accepted by `LLMRequest` construction.
/// From reference/packages/llm/src/schema/messages.ts (`SystemPart.content`)
#[derive(Debug, Clone)]
pub enum SystemPartRef {
    String(String),
    One(SystemPart),
    Many(Vec<SystemPart>),
}

impl From<String> for SystemPartRef {
    fn from(value: String) -> Self {
        SystemPartRef::String(value)
    }
}

impl From<&str> for SystemPartRef {
    fn from(value: &str) -> Self {
        SystemPartRef::String(value.to_string())
    }
}

impl From<SystemPart> for SystemPartRef {
    fn from(value: SystemPart) -> Self {
        SystemPartRef::One(value)
    }
}

impl From<Vec<SystemPart>> for SystemPartRef {
    fn from(value: Vec<SystemPart>) -> Self {
        SystemPartRef::Many(value)
    }
}

/// `TextPart` — `{ type: "text", text, cache?, metadata?, providerMetadata? }`.
/// From reference/packages/llm/src/schema/messages.ts (`TextPart`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TextPart {
    pub fn make(text: impl Into<String>) -> Self {
        Self { part_type: "text".to_string(), text: text.into(), cache: None, metadata: None, provider_metadata: None }
    }
}

/// `MediaPart` — `{ type: "media", mediaType, data, filename?, metadata? }`.
/// From reference/packages/llm/src/schema/messages.ts (`MediaPart`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaPart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub data: MediaData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

/// Media data is either a base64 string or raw bytes.
/// From reference/packages/llm/src/schema/messages.ts (`MediaPart.data`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MediaData {
    Base64(String),
    Bytes(Vec<u8>),
}

/// `ToolTextContent` — `{ type: "text", text }`.
/// From reference/packages/schema/src/llm.ts (`ToolTextContent`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolTextContent {
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: String,
}

/// `ToolFileContent` — `{ type: "file", uri, mime, name? }`.
/// From reference/packages/schema/src/llm.ts (`ToolFileContent`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolFileContent {
    #[serde(rename = "type")]
    pub part_type: String,
    pub uri: String,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `ToolContent` — text or file.
/// From reference/packages/schema/src/llm.ts (`ToolContent`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
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

/// `ToolResultValue` — `{ type: "json"|"text"|"error"|"content", value }`.
/// From reference/packages/llm/src/schema/messages.ts (`ToolResultValue`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolResultValue {
    #[serde(rename = "json")]
    Json { value: Value },
    #[serde(rename = "text")]
    Text { value: Value },
    #[serde(rename = "error")]
    Error { value: Value },
    #[serde(rename = "content")]
    Content { value: Vec<ToolContent> },
}

impl ToolResultValue {
    pub fn is_record(value: &Value) -> bool {
        value.is_object()
    }

    /// `ToolResultValue.is(value)` — guard for the tagged shape.
    pub fn is(value: &Value) -> bool {
        let Some(obj) = value.as_object() else { return false };
        let Some(kind) = obj.get("type").and_then(Value::as_str) else { return false };
        matches!(kind, "json" | "text" | "error" | "content") && obj.contains_key("value")
    }

    /// `ToolResultValue.make(value, type = "json")`.
    pub fn make(value: Value, result_type: Option<&str>) -> ToolResultValue {
        if Self::is(&value) {
            return serde_json::from_value(value).unwrap_or(ToolResultValue::Json { value: Value::Null });
        }
        match result_type {
            Some("content") => {
                let array = value.as_array().cloned().unwrap_or_default();
                let content = array
                    .into_iter()
                    .filter_map(|item| serde_json::from_value::<ToolContent>(item).ok())
                    .collect();
                ToolResultValue::Content { value: content }
            }
            _ => ToolResultValue::Json { value },
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, ToolResultValue::Error { .. })
    }

    pub fn kind(&self) -> &'static str {
        match self {
            ToolResultValue::Json { .. } => "json",
            ToolResultValue::Text { .. } => "text",
            ToolResultValue::Error { .. } => "error",
            ToolResultValue::Content { .. } => "content",
        }
    }
}

/// `ToolOutput` — `{ structured, content }`.
/// From reference/packages/llm/src/schema/messages.ts (`ToolOutput`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutput {
    pub structured: Value,
    pub content: Vec<ToolContent>,
}

impl ToolOutput {
    pub fn make(structured: Value, content: Vec<ToolContent>) -> Self {
        Self { structured, content }
    }

    pub fn from_result_value(result: &ToolResultValue) -> Option<ToolOutput> {
        match result {
            ToolResultValue::Json { value } => Some(ToolOutput { structured: value.clone(), content: vec![] }),
            ToolResultValue::Text { value } => Some(ToolOutput {
                structured: Value::Null,
                content: vec![ToolContent::Text { text: tool_result_text(value) }],
            }),
            ToolResultValue::Content { value } => Some(ToolOutput { structured: Value::Null, content: value.clone() }),
            ToolResultValue::Error { .. } => None,
        }
    }

    pub fn to_result_value(&self) -> ToolResultValue {
        if self.content.is_empty() {
            return ToolResultValue::Json { value: self.structured.clone() };
        }
        if self.content.len() == 1 {
            if let ToolContent::Text { text } = &self.content[0] {
                return ToolResultValue::Text { value: Value::String(text.clone()) };
            }
        }
        ToolResultValue::Content { value: self.content.clone() }
    }
}

fn tool_result_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => {
            if other.is_null() {
                "null".to_string()
            } else {
                serde_json::to_string(other).unwrap_or_else(|_| other.to_string())
            }
        }
    }
}

/// `ToolCallPart` — `{ type: "tool-call", id, name, input, providerExecuted?, metadata?, providerMetadata? }`.
/// From reference/packages/llm/src/schema/messages.ts (`ToolCallPart`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallPart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub id: String,
    pub name: String,
    pub input: Value,
    #[serde(rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolCallPart {
    pub fn make(input: ToolCallPartInput) -> Self {
        Self {
            part_type: "tool-call".to_string(),
            id: input.id,
            name: input.name,
            input: input.input,
            provider_executed: input.provider_executed,
            metadata: input.metadata,
            provider_metadata: input.provider_metadata,
        }
    }
}

pub struct ToolCallPartInput {
    pub id: String,
    pub name: String,
    pub input: Value,
    pub provider_executed: Option<bool>,
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolCallPartInput {
    pub fn new(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        Self { id: id.into(), name: name.into(), input, provider_executed: None, metadata: None, provider_metadata: None }
    }
}

/// `ToolResultPart` — `{ type: "tool-result", id, name, result, providerExecuted?, cache?, metadata?, providerMetadata? }`.
/// From reference/packages/llm/src/schema/messages.ts (`ToolResultPart`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultPart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub id: String,
    pub name: String,
    pub result: ToolResultValue,
    #[serde(rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

pub struct ToolResultPartInput {
    pub id: String,
    pub name: String,
    pub result: Value,
    pub result_type: Option<String>,
    pub provider_executed: Option<bool>,
    pub cache: Option<CacheHint>,
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ToolResultPart {
    /// `ToolResultPart.make(input)` — normalizes `result` through `ToolResultValue.make`.
    pub fn make(input: ToolResultPartInput) -> Self {
        let result = ToolResultValue::make(input.result, input.result_type.as_deref());
        Self {
            part_type: "tool-result".to_string(),
            id: input.id,
            name: input.name,
            result,
            provider_executed: input.provider_executed,
            cache: input.cache,
            metadata: input.metadata,
            provider_metadata: input.provider_metadata,
        }
    }
}

/// `ReasoningPart` — `{ type: "reasoning", text, encrypted?, metadata?, providerMetadata? }`.
/// From reference/packages/llm/src/schema/messages.ts (`ReasoningPart`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningPart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// `ContentPart` — tagged union of the five part kinds.
/// From reference/packages/llm/src/schema/messages.ts (`ContentPart`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache: Option<CacheHint>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Map<String, Value>>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "media")]
    Media {
        #[serde(rename = "mediaType")]
        media_type: String,
        data: MediaData,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Map<String, Value>>,
    },
    #[serde(rename = "tool-call")]
    ToolCall {
        id: String,
        name: String,
        input: Value,
        #[serde(rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Map<String, Value>>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "tool-result")]
    ToolResult {
        id: String,
        name: String,
        result: ToolResultValue,
        #[serde(rename = "providerExecuted", skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache: Option<CacheHint>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Map<String, Value>>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        encrypted: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Map<String, Value>>,
        #[serde(rename = "providerMetadata", skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> ContentPart {
        ContentPart::Text { text: text.into(), cache: None, metadata: None, provider_metadata: None }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            ContentPart::Text { .. } => "text",
            ContentPart::Media { .. } => "media",
            ContentPart::ToolCall { .. } => "tool-call",
            ContentPart::ToolResult { .. } => "tool-result",
            ContentPart::Reasoning { .. } => "reasoning",
        }
    }
}

/// `Message` — `{ id?, role, content, metadata?, native? }`.
/// From reference/packages/llm/src/schema/messages.ts (`Message`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Content input for message constructors: a string, a part, or a list.
/// From reference/packages/llm/src/schema/messages.ts (`Message.ContentInput`)
#[derive(Debug, Clone)]
pub enum ContentInput {
    String(String),
    One(ContentPart),
    Many(Vec<ContentPart>),
}

impl From<String> for ContentInput {
    fn from(value: String) -> Self {
        ContentInput::String(value)
    }
}

impl From<&str> for ContentInput {
    fn from(value: &str) -> Self {
        ContentInput::String(value.to_string())
    }
}

impl From<ContentPart> for ContentInput {
    fn from(value: ContentPart) -> Self {
        ContentInput::One(value)
    }
}

impl From<Vec<ContentPart>> for ContentInput {
    fn from(value: Vec<ContentPart>) -> Self {
        ContentInput::Many(value)
    }
}

impl Message {
    /// `Message.content(input)` — normalize into a `Vec<ContentPart>`.
    pub fn content_parts(input: &ContentInput) -> Vec<ContentPart> {
        match input {
            ContentInput::String(text) => vec![ContentPart::text(text)],
            ContentInput::One(part) => vec![part.clone()],
            ContentInput::Many(parts) => parts.clone(),
        }
    }

    /// `Message.make(input)`.
    pub fn make(input: MessageInput) -> Message {
        let content = Message::content_parts(&input.content);
        Message {
            id: input.id,
            role: input.role,
            content,
            metadata: input.metadata,
            native: input.native,
        }
    }

    /// `Message.user(content)`.
    pub fn user(content: impl Into<ContentInput>) -> Message {
        Message::make(MessageInput {
            id: None,
            role: MessageRole::User,
            content: content.into(),
            metadata: None,
            native: None,
        })
    }

    /// `Message.assistant(content)`.
    pub fn assistant(content: impl Into<ContentInput>) -> Message {
        Message::make(MessageInput {
            id: None,
            role: MessageRole::Assistant,
            content: content.into(),
            metadata: None,
            native: None,
        })
    }

    /// `Message.system(content)` — chronological system update (text only).
    pub fn system(content: impl Into<ContentInput>) -> Message {
        Message::make(MessageInput {
            id: None,
            role: MessageRole::System,
            content: content.into(),
            metadata: None,
            native: None,
        })
    }

    /// `Message.tool(result)`.
    pub fn tool(result: ToolResultPart) -> Message {
        Message::make(MessageInput {
            id: None,
            role: MessageRole::Tool,
            content: ContentInput::One(ContentPart::from_tool_result(result)),
            metadata: None,
            native: None,
        })
    }
}

pub struct MessageInput {
    pub id: Option<String>,
    pub role: MessageRole,
    pub content: ContentInput,
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub native: Option<serde_json::Map<String, Value>>,
}

impl ContentPart {
    pub fn from_tool_call(part: ToolCallPart) -> ContentPart {
        ContentPart::ToolCall {
            id: part.id,
            name: part.name,
            input: part.input,
            provider_executed: part.provider_executed,
            metadata: part.metadata,
            provider_metadata: part.provider_metadata,
        }
    }

    pub fn from_tool_result(part: ToolResultPart) -> ContentPart {
        ContentPart::ToolResult {
            id: part.id,
            name: part.name,
            result: part.result,
            provider_executed: part.provider_executed,
            cache: part.cache,
            metadata: part.metadata,
            provider_metadata: part.provider_metadata,
        }
    }
}

/// `ToolDefinition` — `{ name, description, inputSchema, outputSchema?, cache?, metadata?, native? }`.
/// From reference/packages/llm/src/schema/messages.ts (`ToolDefinition`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: JsonSchema,
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<JsonSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<serde_json::Map<String, Value>>,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            output_schema: None,
            cache: None,
            metadata: None,
            native: None,
        }
    }
}

/// `ToolChoice` — `{ type: "auto"|"none"|"required"|"tool", name? }`.
/// From reference/packages/llm/src/schema/messages.ts (`ToolChoice`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolChoice {
    #[serde(rename = "type")]
    pub kind: ToolChoiceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceType {
    Auto,
    None,
    Required,
    Tool,
}

impl ToolChoice {
    pub fn named(value: impl Into<String>) -> ToolChoice {
        ToolChoice { kind: ToolChoiceType::Tool, name: Some(value.into()) }
    }

    pub fn is_mode(value: &str) -> bool {
        matches!(value, "auto" | "none" | "required")
    }

    /// `ToolChoice.make(input)` — normalize ergonomic inputs.
    pub fn make(input: ToolChoiceInput) -> ToolChoice {
        match input {
            ToolChoiceInput::Choice(choice) => choice,
            ToolChoiceInput::Definition(def) => ToolChoice::named(def.name),
            ToolChoiceInput::String(value) => {
                if Self::is_mode(&value) {
                    let kind = match value.as_str() {
                        "none" => ToolChoiceType::None,
                        "required" => ToolChoiceType::Required,
                        _ => ToolChoiceType::Auto,
                    };
                    ToolChoice { kind, name: None }
                } else {
                    ToolChoice::named(value)
                }
            }
            ToolChoiceInput::Fields { kind, name } => ToolChoice { kind, name },
        }
    }
}

#[derive(Debug, Clone)]
pub enum ToolChoiceInput {
    Choice(ToolChoice),
    Definition(ToolDefinition),
    String(String),
    Fields { kind: ToolChoiceType, name: Option<String> },
}

impl From<ToolChoice> for ToolChoiceInput {
    fn from(value: ToolChoice) -> Self {
        ToolChoiceInput::Choice(value)
    }
}

impl From<ToolDefinition> for ToolChoiceInput {
    fn from(value: ToolDefinition) -> Self {
        ToolChoiceInput::Definition(value)
    }
}

impl From<&str> for ToolChoiceInput {
    fn from(value: &str) -> Self {
        ToolChoiceInput::String(value.to_string())
    }
}

impl From<String> for ToolChoiceInput {
    fn from(value: String) -> Self {
        ToolChoiceInput::String(value)
    }
}

/// `ResponseFormat` — `{ type: "text" } | { type: "json", schema } | { type: "tool", tool }`.
/// From reference/packages/llm/src/schema/messages.ts (`ResponseFormat`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseFormat {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json")]
    Json { schema: JsonSchema },
    #[serde(rename = "tool")]
    Tool { tool: ToolDefinition },
}

/// `LLMRequest` — canonical request class.
/// From reference/packages/llm/src/schema/messages.ts (`LLMRequest`)
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub id: Option<String>,
    pub model: Model,
    pub system: Vec<SystemPart>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<super::options::ProviderOptions>,
    pub http: Option<HttpOptions>,
    pub response_format: Option<ResponseFormat>,
    pub cache: Option<CachePolicy>,
    pub metadata: Option<serde_json::Map<String, Value>>,
}

impl LlmRequest {
    pub fn new(input: LlmRequestInput) -> LlmRequest {
        LlmRequest {
            id: input.id,
            model: input.model,
            system: input.system,
            messages: input.messages,
            tools: input.tools,
            tool_choice: input.tool_choice,
            generation: input.generation,
            provider_options: input.provider_options,
            http: input.http,
            response_format: input.response_format,
            cache: input.cache,
            metadata: input.metadata,
        }
    }

    /// `LLMRequest.input(request)`.
    pub fn input(&self) -> LlmRequestInput {
        LlmRequestInput {
            id: self.id.clone(),
            model: self.model.clone(),
            system: self.system.clone(),
            messages: self.messages.clone(),
            tools: self.tools.clone(),
            tool_choice: self.tool_choice.clone(),
            generation: self.generation.clone(),
            provider_options: self.provider_options.clone(),
            http: self.http.clone(),
            response_format: self.response_format.clone(),
            cache: self.cache.clone(),
            metadata: self.metadata.clone(),
        }
    }

    /// `LLMRequest.update(request, patch)`.
    pub fn update(request: &LlmRequest, patch: LlmRequestPatch) -> LlmRequest {
        let mut input = request.input();
        if let Some(id) = patch.id {
            input.id = id;
        }
        if let Some(model) = patch.model {
            input.model = model;
        }
        if let Some(system) = patch.system {
            input.system = system;
        }
        if let Some(messages) = patch.messages {
            input.messages = messages;
        }
        if let Some(tools) = patch.tools {
            input.tools = tools;
        }
        if let Some(tool_choice) = patch.tool_choice {
            input.tool_choice = tool_choice;
        }
        if let Some(generation) = patch.generation {
            input.generation = generation;
        }
        if let Some(provider_options) = patch.provider_options {
            input.provider_options = provider_options;
        }
        if let Some(http) = patch.http {
            input.http = http;
        }
        if let Some(response_format) = patch.response_format {
            input.response_format = response_format;
        }
        if let Some(cache) = patch.cache {
            input.cache = cache;
        }
        if let Some(metadata) = patch.metadata {
            input.metadata = metadata;
        }
        LlmRequest::new(input)
    }
}

pub struct LlmRequestInput {
    pub id: Option<String>,
    pub model: Model,
    pub system: Vec<SystemPart>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<super::options::ProviderOptions>,
    pub http: Option<HttpOptions>,
    pub response_format: Option<ResponseFormat>,
    pub cache: Option<CachePolicy>,
    pub metadata: Option<serde_json::Map<String, Value>>,
}

pub struct LlmRequestPatch {
    pub id: Option<Option<String>>,
    pub model: Option<Model>,
    pub system: Option<Vec<SystemPart>>,
    pub messages: Option<Vec<Message>>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub tool_choice: Option<Option<ToolChoice>>,
    pub generation: Option<Option<GenerationOptions>>,
    pub provider_options: Option<Option<super::options::ProviderOptions>>,
    pub http: Option<Option<HttpOptions>>,
    pub response_format: Option<Option<ResponseFormat>>,
    pub cache: Option<Option<CachePolicy>>,
    pub metadata: Option<Option<serde_json::Map<String, Value>>>,
}

impl LlmRequestPatch {
    pub fn empty() -> LlmRequestPatch {
        LlmRequestPatch {
            id: None,
            model: None,
            system: None,
            messages: None,
            tools: None,
            tool_choice: None,
            generation: None,
            provider_options: None,
            http: None,
            response_format: None,
            cache: None,
            metadata: None,
        }
    }
}
