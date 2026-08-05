use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::schema::ModelRef;
use crate::llm::ProviderMetadata;

/// Branded `Session.Message.ID` (`msg_...`). Kept untyped here.
/// /// From reference/packages/schema/src/session-message.ts
pub type MessageID = String;

/// `Session.Error.Unknown`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnknownError {
    #[serde(rename = "type")]
    pub kind: UnknownErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownErrorKind {
    Unknown,
}

impl UnknownError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: UnknownErrorKind::Unknown,
            message: message.into(),
        }
    }
}

/// `Prompt.FileAttachment`
/// /// From reference/packages/schema/src/prompt.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachment {
    pub uri: String,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `Session.Message.Assistant.Tool` provider block.
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProvider {
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_metadata: Option<ProviderMetadata>,
}

/// `Session.Message.ToolState.*`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ToolState {
    #[serde(rename = "pending")]
    Pending { input: String },
    #[serde(rename = "running")]
    Running {
        input: serde_json::Map<String, Value>,
        #[serde(default)]
        structured: serde_json::Map<String, Value>,
        #[serde(default)]
        content: Vec<crate::llm::ToolContent>,
    },
    #[serde(rename = "completed")]
    Completed {
        input: serde_json::Map<String, Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<FileAttachment>>,
        #[serde(default)]
        content: Vec<crate::llm::ToolContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_paths: Option<Vec<String>>,
        #[serde(default)]
        structured: serde_json::Map<String, Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
    #[serde(rename = "error")]
    Error {
        input: serde_json::Map<String, Value>,
        #[serde(default)]
        content: Vec<crate::llm::ToolContent>,
        #[serde(default)]
        structured: serde_json::Map<String, Value>,
        error: UnknownError,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
}

/// `Session.Message.Assistant.Tool`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTool {
    #[serde(rename = "type")]
    pub kind: AssistantContentKind,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ToolProvider>,
    pub state: ToolState,
    pub time: ToolTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTime {
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ran: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pruned: Option<String>,
}

/// `Session.Message.Assistant.Text`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantText {
    #[serde(rename = "type")]
    pub kind: AssistantContentKind,
    pub id: String,
    pub text: String,
}

/// `Session.Message.Assistant.Reasoning`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantReasoning {
    #[serde(rename = "type")]
    pub kind: AssistantContentKind,
    pub id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<ReasoningTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningTime {
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssistantContentKind {
    Text,
    Reasoning,
    Tool,
}

/// `Session.Message.AssistantContent`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AssistantContent {
    Text(AssistantText),
    Reasoning(AssistantReasoning),
    Tool(Box<AssistantTool>),
}

impl AssistantContent {
    pub fn as_tool(&self) -> Option<&AssistantTool> {
        match self {
            Self::Tool(tool) => Some(tool),
            _ => None,
        }
    }
}

/// `Session.Message.Assistant`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assistant {
    pub id: MessageID,
    #[serde(rename = "type")]
    pub kind: MessageKind,
    pub agent: String,
    pub model: ModelRef,
    #[serde(default)]
    pub content: Vec<AssistantContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<UnknownError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<AssistantSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub time: MessageTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: CacheTokens,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheTokens {
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTime {
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
}

/// `Session.Message.User`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: MessageID,
    #[serde(rename = "type")]
    pub kind: MessageKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<AgentAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub time: MessageTime,
}

/// `Prompt.AgentAttachment`
/// /// From reference/packages/schema/src/prompt.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAttachment {
    pub name: String,
}

/// `Session.Message.Synthetic`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Synthetic {
    pub id: MessageID,
    #[serde(rename = "type")]
    pub kind: MessageKind,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub time: MessageTime,
}

/// `Session.Message.System`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct System {
    pub id: MessageID,
    #[serde(rename = "type")]
    pub kind: MessageKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub time: MessageTime,
}

/// `Session.Message.Shell`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shell {
    pub id: MessageID,
    #[serde(rename = "type")]
    pub kind: MessageKind,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub command: String,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub time: ShellTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellTime {
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
}

/// `Session.Message.Compaction`
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Compaction {
    pub id: MessageID,
    #[serde(rename = "type")]
    pub kind: MessageKind,
    pub reason: CompactionReason,
    pub summary: String,
    pub recent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    pub time: MessageTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompactionReason {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageKind {
    #[serde(rename = "agent-switched")]
    AgentSwitched,
    #[serde(rename = "model-switched")]
    ModelSwitched,
    User,
    Synthetic,
    System,
    Shell,
    Assistant,
    Compaction,
}

/// `Session.Message` tagged union.
/// /// From reference/packages/schema/src/session-message.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SessionMessage {
    #[serde(rename = "agent-switched")]
    AgentSwitched {
        id: MessageID,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Map<String, Value>>,
        time: MessageTime,
        agent: String,
    },
    #[serde(rename = "model-switched")]
    ModelSwitched {
        id: MessageID,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Map<String, Value>>,
        time: MessageTime,
        model: ModelRef,
    },
    User(User),
    Synthetic(Synthetic),
    System(System),
    Shell(Shell),
    Assistant(Box<Assistant>),
    Compaction(Compaction),
}
