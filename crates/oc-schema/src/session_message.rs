//! From reference/packages/schema/src/session-message.ts

use crate::identifier::ascending;
use crate::llm::{ProviderMetadata, ToolContent};
use crate::model;
use crate::prompt::{AgentAttachment, FileAttachment};
use crate::schema::{DateTimeUtc, Finite, RelativePath};
use crate::session_id::SessionID;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `Session.Message.ID` — starts with `msg_`.
pub type ID = String;

/// `Session.Message.ID.create()`.
pub fn create_id() -> ID {
    format!("msg_{}", ascending())
}

/// `Session.Error.Unknown`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UnknownError {
    #[serde(rename = "type")]
    pub r#type: UnknownErrorType,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum UnknownErrorType {
    #[serde(rename = "unknown")]
    Value,
}

/// Shared `{ created }` time shape.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TimeCreated {
    pub created: DateTimeUtc,
}

/// Shared `{ created, completed? }` time shape.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TimeCompleted {
    pub created: DateTimeUtc,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub completed: Option<DateTimeUtc>,
}

/// Token usage shape shared by assistant messages, sessions, and step events.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TokenUsage {
    pub input: Finite,
    pub output: Finite,
    pub reasoning: Finite,
    pub cache: TokenCache,
}

/// `tokens.cache`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TokenCache {
    pub read: Finite,
    pub write: Finite,
}

/// `Session.Message.AgentSwitched`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentSwitched {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCreated,
    #[serde(rename = "type")]
    pub r#type: AgentSwitchedType,
    pub agent: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AgentSwitchedType {
    #[serde(rename = "agent-switched")]
    Value,
}

/// `Session.Message.ModelSwitched`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModelSwitched {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCreated,
    #[serde(rename = "type")]
    pub r#type: ModelSwitchedType,
    pub model: model::Ref,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ModelSwitchedType {
    #[serde(rename = "model-switched")]
    Value,
}

/// `Session.Message.User`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct User {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCreated,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agents: Option<Vec<AgentAttachment>>,
    #[serde(rename = "type")]
    pub r#type: UserType,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum UserType {
    #[serde(rename = "user")]
    Value,
}

/// `Session.Message.Synthetic`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Synthetic {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCreated,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub text: String,
    #[serde(rename = "type")]
    pub r#type: SyntheticType,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SyntheticType {
    #[serde(rename = "synthetic")]
    Value,
}

/// `Session.Message.System`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct System {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCreated,
    #[serde(rename = "type")]
    pub r#type: SystemType,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SystemType {
    #[serde(rename = "system")]
    Value,
}

/// `Session.Message.Shell`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Shell {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCompleted,
    #[serde(rename = "type")]
    pub r#type: ShellType,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub command: String,
    pub output: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ShellType {
    #[serde(rename = "shell")]
    Value,
}

/// `Session.Message.ToolState.Pending` — note `input` is a string here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStatePending {
    pub status: ToolStatePendingStatus,
    pub input: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStatePendingStatus {
    #[serde(rename = "pending")]
    Value,
}

/// `Session.Message.ToolState.Running`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateRunning {
    pub status: ToolStateRunningStatus,
    pub input: IndexMap<String, Value>,
    pub structured: IndexMap<String, Value>,
    pub content: Vec<ToolContent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStateRunningStatus {
    #[serde(rename = "running")]
    Value,
}

/// `Session.Message.ToolState.Completed`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateCompleted {
    pub status: ToolStateCompletedStatus,
    pub input: IndexMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub attachments: Option<Vec<FileAttachment>>,
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[serde(rename = "outputPaths")]
    pub output_paths: Option<Vec<String>>,
    pub structured: IndexMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStateCompletedStatus {
    #[serde(rename = "completed")]
    Value,
}

/// `Session.Message.ToolState.Error`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateError {
    pub status: ToolStateErrorStatus,
    pub input: IndexMap<String, Value>,
    pub content: Vec<ToolContent>,
    pub structured: IndexMap<String, Value>,
    pub error: UnknownError,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStateErrorStatus {
    #[serde(rename = "error")]
    Value,
}

/// `Session.Message.ToolState` — tagged union on `status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ToolState {
    Pending(ToolStatePending),
    Running(ToolStateRunning),
    Completed(ToolStateCompleted),
    Error(ToolStateError),
}

/// `Session.Message.Assistant.Tool.provider`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantToolProvider {
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[serde(rename = "resultMetadata")]
    pub result_metadata: Option<ProviderMetadata>,
}

/// `Session.Message.Assistant.Tool.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantToolTime {
    pub created: DateTimeUtc,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ran: Option<DateTimeUtc>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub completed: Option<DateTimeUtc>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pruned: Option<DateTimeUtc>,
}

/// `Session.Message.Assistant.Tool`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantTool {
    #[serde(rename = "type")]
    pub r#type: AssistantToolType,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider: Option<AssistantToolProvider>,
    pub state: ToolState,
    pub time: AssistantToolTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AssistantToolType {
    #[serde(rename = "tool")]
    Value,
}

/// `Session.Message.Assistant.Text`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantText {
    #[serde(rename = "type")]
    pub r#type: AssistantTextType,
    pub id: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AssistantTextType {
    #[serde(rename = "text")]
    Value,
}

/// `Session.Message.Assistant.Reasoning`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantReasoning {
    #[serde(rename = "type")]
    pub r#type: AssistantReasoningType,
    pub id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[serde(rename = "providerMetadata")]
    pub provider_metadata: Option<ProviderMetadata>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub time: Option<TimeCompleted>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AssistantReasoningType {
    #[serde(rename = "reasoning")]
    Value,
}

/// `Session.Message.Assistant.Content` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum AssistantContent {
    Text(AssistantText),
    Reasoning(AssistantReasoning),
    Tool(AssistantTool),
}

/// `Session.Message.Assistant.snapshot`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantSnapshot {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub files: Option<Vec<RelativePath>>,
}

/// `Session.Message.Assistant`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Assistant {
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCompleted,
    #[serde(rename = "type")]
    pub r#type: AssistantType,
    pub agent: String,
    pub model: model::Ref,
    pub content: Vec<AssistantContent>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<AssistantSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub finish: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cost: Option<Finite>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tokens: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<UnknownError>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AssistantType {
    #[serde(rename = "assistant")]
    Value,
}

/// `Session.Message.Compaction.reason`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum CompactionReason {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "manual")]
    Manual,
}

/// `Session.Message.Compaction`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Compaction {
    #[serde(rename = "type")]
    pub r#type: CompactionType,
    pub reason: CompactionReason,
    pub summary: String,
    pub recent: String,
    pub id: ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, Value>>,
    pub time: TimeCreated,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CompactionType {
    #[serde(rename = "compaction")]
    Value,
}

/// `Session.Message` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Message {
    AgentSwitched(AgentSwitched),
    ModelSwitched(ModelSwitched),
    User(User),
    Synthetic(Synthetic),
    System(System),
    Shell(Shell),
    Assistant(Assistant),
    Compaction(Compaction),
}

/// `Session.Message.Type`.
pub type Type = String;
