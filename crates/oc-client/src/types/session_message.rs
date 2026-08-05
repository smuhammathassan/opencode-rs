//! Session message types.
//! From reference/packages/schema/src/session-message.ts.

// TODO(integration): promote to oc-schema.
use crate::types::model::ModelRef;
use crate::types::prompt::{PromptAgentAttachment, PromptFileAttachment};
use crate::types::schema::{DateTimeMillis, JsonValue, Order};
use std::collections::HashMap;

/// `SessionMessagesResponse` — `{ data, cursor }` from `session.messages`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessagesResponse {
    pub data: Vec<SessionMessage>,
    pub cursor: crate::types::session::ResponseCursor,
}

/// `MessagesListInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessagesListInput {
    pub session_id: String,
    pub limit: Option<u64>,
    pub order: Option<Order>,
    pub cursor: Option<String>,
}

/// `Session.Error.Unknown`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUnknownError {
    #[serde(rename = "type")]
    pub kind: SessionErrorType,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionErrorType {
    Unknown,
}

/// `LLM.ToolContent`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "file")]
    File {
        uri: String,
        mime: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

/// `Session.Message.ToolState` — tagged on `status`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
pub enum ToolState {
    #[serde(rename = "pending")]
    Pending { input: String },
    #[serde(rename = "running")]
    Running {
        input: HashMap<String, JsonValue>,
        structured: HashMap<String, JsonValue>,
        content: Vec<ToolContent>,
    },
    #[serde(rename = "completed")]
    Completed {
        input: HashMap<String, JsonValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<PromptFileAttachment>>,
        content: Vec<ToolContent>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "outputPaths"
        )]
        output_paths: Option<Vec<String>>,
        structured: HashMap<String, JsonValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<JsonValue>,
    },
    #[serde(rename = "error")]
    Error {
        input: HashMap<String, JsonValue>,
        content: Vec<ToolContent>,
        structured: HashMap<String, JsonValue>,
        error: SessionUnknownError,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<JsonValue>,
    },
}

/// `Session.Message.Assistant.Tool`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTool {
    #[serde(rename = "type")]
    pub kind: AssistantToolType,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub provider: Option<AssistantToolProvider>,
    pub state: ToolState,
    pub time: AssistantToolTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssistantToolType {
    Tool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantToolProvider {
    pub executed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, HashMap<String, JsonValue>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_metadata: Option<HashMap<String, HashMap<String, JsonValue>>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantToolTime {
    pub created: DateTimeMillis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ran: Option<DateTimeMillis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<DateTimeMillis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pruned: Option<DateTimeMillis>,
}

/// `Session.Message.AssistantContent` — tagged on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum AssistantContent {
    #[serde(rename = "text")]
    Text { id: String, text: String },
    #[serde(rename = "reasoning")]
    Reasoning {
        id: String,
        text: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "providerMetadata"
        )]
        provider_metadata: Option<HashMap<String, HashMap<String, JsonValue>>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<MessageTimeCompleted>,
    },
    #[serde(rename = "tool")]
    Tool(AssistantTool),
}

/// Common `time` for messages: `{ created, completed? }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTimeCompleted {
    pub created: DateTimeMillis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<DateTimeMillis>,
}

/// Common `time` for messages: `{ created }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTime {
    pub created: DateTimeMillis,
}

/// Token usage counters.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: TokenCache,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCache {
    pub read: f64,
    pub write: f64,
}

/// `Session.Message` — tagged union on `type`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "type")]
pub enum SessionMessage {
    #[serde(rename = "agent-switched")]
    AgentSwitched {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTime,
        agent: String,
    },
    #[serde(rename = "model-switched")]
    ModelSwitched {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTime,
        model: ModelRef,
    },
    #[serde(rename = "user")]
    User {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTime,
        text: String,
        #[serde(default)]
        files: Option<Vec<PromptFileAttachment>>,
        #[serde(default)]
        agents: Option<Vec<PromptAgentAttachment>>,
    },
    #[serde(rename = "synthetic")]
    Synthetic {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTime,
        session_id: String,
        text: String,
    },
    #[serde(rename = "system")]
    System {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTime,
        text: String,
    },
    #[serde(rename = "shell")]
    Shell {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTimeCompleted,
        call_id: String,
        command: String,
        output: String,
    },
    #[serde(rename = "assistant")]
    Assistant {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTimeCompleted,
        agent: String,
        model: ModelRef,
        content: Vec<AssistantContent>,
        #[serde(default)]
        snapshot: Option<AssistantSnapshot>,
        #[serde(default)]
        finish: Option<String>,
        #[serde(default)]
        cost: Option<f64>,
        #[serde(default)]
        tokens: Option<Tokens>,
        #[serde(default)]
        error: Option<SessionUnknownError>,
    },
    #[serde(rename = "compaction")]
    Compaction {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        time: MessageTime,
        reason: CompactionReason,
        summary: String,
        recent: String,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompactionReason {
    Auto,
    Manual,
}
