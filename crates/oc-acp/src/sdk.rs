//! Abstraction over the opencode SDK (`@opencode-ai/sdk/v2`) that the ACP
//! service talks to.
//!
//! The reference implementation drives a generated HTTP client (`OpencodeClient`)
//! for session/config/provider/command/mcp/permission operations and a global
//! event stream. This module defines the minimal surface the ACP service needs
//! plus the data shapes (`Session`, `Message`, `Part`, `Event`, ...) that flow
//! through it. The shapes mirror `reference/packages/sdk/js/src/v2/gen/types.gen.ts`.
//!
//! TODO(integration): oc-server will provide the concrete HTTP implementation of
//! [`OpencodeClient`]. The data types here should be reconciled with `oc-schema`.

use async_trait::async_trait;
use futures::stream::BoxStream;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// An SDK error payload. The reference treats SDK errors as opaque objects that
/// are inspected via duck typing (see `isAuthRequired`/`findProviderID` in
/// reference/packages/opencode/src/acp/service.ts), so the raw JSON is kept.
pub type SdkError = Value;

// ---------------------------------------------------------------------------
// opencode data shapes (subset used by ACP)
// ---------------------------------------------------------------------------

/// `Session` from the opencode SDK.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub directory: String,
    pub title: String,
    #[serde(default)]
    pub time: SessionTime,
}

/// Session timestamps (epoch milliseconds).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTime {
    pub created: i64,
    pub updated: i64,
}

/// `Tokens` from an assistant message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache: CacheTokens,
}

/// Cached token counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheTokens {
    pub read: u64,
    pub write: u64,
}

/// `UserMessage` (subset).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    #[serde(default)]
    pub model: Option<UserMessageModel>,
    #[serde(default)]
    pub agent: Option<String>,
}

/// `UserMessage.model`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessageModel {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub variant: Option<String>,
}

/// `AssistantMessage` (subset).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    pub cost: f64,
    pub tokens: Tokens,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub path: Option<MessagePath>,
    #[serde(default)]
    pub model: Option<UserMessageModel>,
}

/// `AssistantMessage.path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePath {
    pub cwd: String,
    pub root: String,
}

/// `Message` = `UserMessage | AssistantMessage`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    User(UserMessage),
    Assistant(Box<AssistantMessage>),
}

impl Message {
    /// The message role.
    pub fn role(&self) -> &str {
        match self {
            Message::User(message) => &message.role,
            Message::Assistant(message) => &message.role,
        }
    }

    /// The message id.
    pub fn id(&self) -> &str {
        match self {
            Message::User(message) => &message.id,
            Message::Assistant(message) => &message.id,
        }
    }

    /// The session id the message belongs to.
    pub fn session_id(&self) -> &str {
        match self {
            Message::User(message) => &message.session_id,
            Message::Assistant(message) => &message.session_id,
        }
    }

    /// The assistant message `path.cwd`, if any.
    pub fn assistant_cwd(&self) -> Option<&str> {
        match self {
            Message::Assistant(message) => message.path.as_ref().map(|path| path.cwd.as_str()),
            _ => None,
        }
    }
}

/// A `TextPart`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub text: String,
    #[serde(default)]
    pub synthetic: Option<bool>,
    #[serde(default)]
    pub ignored: Option<bool>,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
}

/// A `FilePart`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub mime: String,
    #[serde(default)]
    pub filename: Option<String>,
    pub url: String,
}

/// A `ReasoningPart`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub text: String,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
}

/// `ToolState.pending`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatePending {
    pub input: Map<String, Value>,
    pub raw: String,
}

/// `ToolState.running`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateRunning {
    pub input: Map<String, Value>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
}

/// `ToolState.completed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateCompleted {
    pub input: Map<String, Value>,
    pub output: String,
    pub title: String,
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub attachments: Option<Vec<FilePart>>,
}

/// `ToolState.error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateError {
    pub input: Map<String, Value>,
    pub error: String,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
}

/// `ToolState`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolState {
    Pending(ToolStatePending),
    Running(ToolStateRunning),
    Completed(ToolStateCompleted),
    Error(ToolStateError),
}

/// A `ToolPart`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub call_id: String,
    pub tool: String,
    pub state: ToolState,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
}

/// Any message part produced by the opencode session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    Text(TextPart),
    File(FilePart),
    Reasoning(ReasoningPart),
    Tool(Box<ToolPart>),
    Other(Value),
}

impl Part {
    /// The `type` discriminator.
    pub fn part_type(&self) -> &str {
        match self {
            Part::Text(_) => "text",
            Part::File(_) => "file",
            Part::Reasoning(_) => "reasoning",
            Part::Tool(_) => "tool",
            Part::Other(value) => value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        }
    }

    /// The part id.
    pub fn id(&self) -> Option<&str> {
        match self {
            Part::Text(part) => Some(&part.id),
            Part::File(part) => Some(&part.id),
            Part::Reasoning(part) => Some(&part.id),
            Part::Tool(part) => Some(&part.id),
            Part::Other(value) => value.get("id").and_then(Value::as_str),
        }
    }

    /// The message id this part belongs to.
    pub fn message_id(&self) -> Option<&str> {
        match self {
            Part::Text(part) => Some(&part.message_id),
            Part::File(part) => Some(&part.message_id),
            Part::Reasoning(part) => Some(&part.message_id),
            Part::Tool(part) => Some(&part.message_id),
            Part::Other(value) => value.get("messageID").and_then(Value::as_str),
        }
    }

    /// `ignored` flag for text parts.
    pub fn ignored(&self) -> Option<bool> {
        match self {
            Part::Text(part) => part.ignored,
            Part::Other(value) => value.get("ignored").and_then(Value::as_bool),
            _ => None,
        }
    }

    /// `callID` for tool parts.
    pub fn call_id(&self) -> Option<&str> {
        match self {
            Part::Tool(part) => Some(&part.call_id),
            Part::Other(value) => value.get("callID").and_then(Value::as_str),
            _ => None,
        }
    }

    /// Free-form `metadata` attached to the part.
    pub fn metadata(&self) -> Option<&Map<String, Value>> {
        match self {
            Part::Text(part) => part.metadata.as_ref(),
            Part::Reasoning(part) => part.metadata.as_ref(),
            Part::Tool(part) => part.metadata.as_ref(),
            _ => None,
        }
    }
}

/// `{ info: Message; parts: Part[] }` returned by session/message endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessageResponse {
    pub info: Message,
    pub parts: Vec<Part>,
}

/// `permission.asked` event properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAskedProperties {
    pub id: String,
    pub session_id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub metadata: Map<String, Value>,
    pub always: Vec<String>,
    #[serde(default)]
    pub tool: Option<PermissionTool>,
}

/// `permission.asked` tool reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionTool {
    pub message_id: String,
    pub call_id: String,
}

/// `message.part.updated` event properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePartUpdatedProperties {
    pub session_id: String,
    pub part: Part,
}

/// `message.part.delta` event properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePartDeltaProperties {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    pub field: String,
    pub delta: String,
}

/// Events consumed by the ACP subscription.
///
/// From `Event` in reference/packages/sdk/js/src/v2/gen/types.gen.ts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    #[serde(rename = "permission.asked")]
    PermissionAsked {
        id: String,
        properties: PermissionAskedProperties,
    },
    #[serde(rename = "message.part.updated")]
    MessagePartUpdated {
        id: String,
        properties: MessagePartUpdatedProperties,
    },
    #[serde(rename = "message.part.delta")]
    MessagePartDelta {
        id: String,
        properties: MessagePartDeltaProperties,
    },
    Other(Value),
}

/// `Config` (only the `model` field is consumed).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub model: Option<String>,
}

/// `config/providers` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProviders {
    pub providers: Vec<ProviderInfo>,
}

/// `Provider` info.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub options: Map<String, Value>,
    pub models: IndexMap<String, ModelInfo>,
}

/// `Model` info.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    #[serde(default)]
    pub variants: Option<IndexMap<String, Map<String, Value>>>,
    #[serde(default)]
    pub limit: Option<ModelLimit>,
}

/// `ProviderLimit`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLimit {
    pub context: f64,
    pub output: f64,
}

/// `Agent` info from `/agent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub mode: String,
    #[serde(default)]
    pub hidden: Option<bool>,
}

/// `Skill` info from `/skill`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub content: String,
}

/// `Command` info from `/command`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub template: Value,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
}

// ---------------------------------------------------------------------------
// Request payloads sent to the opencode SDK
// ---------------------------------------------------------------------------

/// Body for `session.create`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateRequest {
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub model: SessionCreateModel,
}

/// `session.create` model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateModel {
    pub provider_id: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// `session.prompt` body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    pub session_id: String,
    pub model: ModelSelection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub parts: Vec<PromptPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub directory: String,
}

/// `{ providerID, modelID }` model reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub provider_id: String,
    pub model_id: String,
}

/// An opencode prompt part (text or file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptPart {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        synthetic: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ignored: Option<bool>,
    },
    File {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        mime: String,
    },
}

/// `session.command` body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRequest {
    pub session_id: String,
    pub command: String,
    pub arguments: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub directory: String,
}

/// `session.summarize` body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarizeRequest {
    pub session_id: String,
    pub directory: String,
    pub provider_id: String,
    pub model_id: String,
}

// ---------------------------------------------------------------------------
// Client trait
// ---------------------------------------------------------------------------

/// The subset of the opencode SDK consumed by the ACP service.
///
/// TODO(integration): implement for the `oc-client` HTTP client once it exists.
#[async_trait]
pub trait OpencodeClient: Send + Sync {
    /// The global event stream. Each item carries an optional payload, mirroring
    /// the SDK's `{ payload?: Event }` envelope; the stream ends and is
    /// re-established by the caller.
    fn global_event(&self) -> BoxStream<'static, Option<Event>>;

    async fn session_create(&self, request: SessionCreateRequest) -> Result<Session, SdkError>;
    async fn session_get(&self, directory: &str, session_id: &str) -> Result<Session, SdkError>;
    async fn session_messages(
        &self,
        directory: &str,
        session_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<SessionMessageResponse>, SdkError>;
    async fn session_message(
        &self,
        directory: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<SessionMessageResponse, SdkError>;
    async fn session_list(&self, directory: Option<&str>) -> Result<Vec<Session>, SdkError>;
    async fn session_abort(&self, directory: &str, session_id: &str) -> Result<(), SdkError>;
    async fn session_prompt(&self, request: PromptRequest) -> Result<AssistantMessage, SdkError>;
    async fn session_command(&self, request: CommandRequest) -> Result<AssistantMessage, SdkError>;
    async fn session_summarize(&self, request: SummarizeRequest) -> Result<bool, SdkError>;
    async fn session_fork(&self, directory: &str, session_id: &str) -> Result<Session, SdkError>;
    async fn config_providers(&self, directory: &str) -> Result<ConfigProviders, SdkError>;
    async fn config_get(&self, directory: &str) -> Result<Config, SdkError>;
    async fn app_agents(&self, directory: &str) -> Result<Vec<AgentInfo>, SdkError>;
    async fn app_skills(&self, directory: &str) -> Result<Vec<SkillInfo>, SdkError>;
    async fn command_list(&self, directory: &str) -> Result<Vec<CommandInfo>, SdkError>;
    async fn mcp_add(&self, directory: &str, name: &str, config: Value) -> Result<Value, SdkError>;
    async fn permission_reply(
        &self,
        request_id: &str,
        reply: &str,
        directory: &str,
    ) -> Result<bool, SdkError>;
}
