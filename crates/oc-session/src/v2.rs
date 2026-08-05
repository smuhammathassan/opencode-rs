/// From reference/packages/schema/src/session-message.ts and
/// reference/packages/schema/src/prompt.ts — the "next" (V2) session message
/// model and prompt attachments.
///
/// TODO(integration): promote to oc-schema once the schema crate lands.
use serde::{Deserialize, Serialize};

use crate::JsonMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachment {
    pub uri: String,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAttachment {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

/// From reference `prompt.ts:Prompt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<AgentAttachment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnknownError {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTime {
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageBase {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    pub time: MessageTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageBaseId {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSwitched {
    #[serde(flatten)]
    pub base: MessageBase,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSwitched {
    #[serde(flatten)]
    pub base: MessageBase,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub model: ModelRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    #[serde(flatten)]
    pub base: MessageBase,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<AgentAttachment>>,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Synthetic {
    #[serde(flatten)]
    pub base: MessageBase,
    pub session_id: String,
    pub text: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct System {
    #[serde(flatten)]
    pub base: MessageBase,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shell {
    #[serde(flatten)]
    pub base: MessageBaseId,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub command: String,
    pub output: String,
    pub time: ShellTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellTime {
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatePending {
    #[serde(rename = "status", default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateRunning {
    #[serde(rename = "status", default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    pub input: JsonMap,
    pub structured: JsonMap,
    pub content: Vec<ToolContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateCompleted {
    #[serde(rename = "status", default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    pub input: JsonMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<FileAttachment>>,
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_paths: Option<Vec<String>>,
    pub structured: JsonMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateError {
    #[serde(rename = "status", default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    pub input: JsonMap,
    pub content: Vec<ToolContent>,
    pub structured: JsonMap,
    pub error: UnknownError,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ToolState {
    #[serde(rename = "pending")]
    Pending(ToolStatePending),
    #[serde(rename = "running")]
    Running(ToolStateRunning),
    #[serde(rename = "completed")]
    Completed(ToolStateCompleted),
    #[serde(rename = "error")]
    Error(ToolStateError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTextContent {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFileContent {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub uri: String,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text(ToolTextContent),
    #[serde(rename = "file")]
    File(ToolFileContent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantToolTime {
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ran: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pruned: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTool {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderInfo>,
    pub state: ToolState,
    pub time: AssistantToolTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantText {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantReasoning {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<AssistantReasoningTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantReasoningTime {
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AssistantContent {
    #[serde(rename = "text")]
    Text(AssistantText),
    #[serde(rename = "reasoning")]
    Reasoning(AssistantReasoning),
    #[serde(rename = "tool")]
    Tool(Box<AssistantTool>),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: CacheTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheTokens {
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTime {
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assistant {
    #[serde(flatten)]
    pub base: MessageBaseId,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub agent: String,
    pub model: ModelRef,
    pub content: Vec<AssistantContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<AssistantSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<AssistantTokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<UnknownError>,
    pub time: AssistantTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Compaction {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    pub reason: String,
    pub summary: String,
    pub recent: String,
    #[serde(flatten)]
    pub base: MessageBase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "agent-switched")]
    AgentSwitched(AgentSwitched),
    #[serde(rename = "model-switched")]
    ModelSwitched(ModelSwitched),
    #[serde(rename = "user")]
    User(User),
    #[serde(rename = "synthetic")]
    Synthetic(Synthetic),
    #[serde(rename = "system")]
    System(System),
    #[serde(rename = "shell")]
    Shell(Box<Shell>),
    #[serde(rename = "assistant")]
    Assistant(Box<Assistant>),
    #[serde(rename = "compaction")]
    Compaction(Compaction),
}

impl Message {
    pub fn id(&self) -> &str {
        match self {
            Message::AgentSwitched(m) => &m.base.id,
            Message::ModelSwitched(m) => &m.base.id,
            Message::User(m) => &m.base.id,
            Message::Synthetic(m) => &m.base.id,
            Message::System(m) => &m.base.id,
            Message::Shell(m) => &m.base.id,
            Message::Assistant(m) => &m.base.id,
            Message::Compaction(m) => &m.base.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durable: Option<Durable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<JsonMap>,
    pub data: EventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Durable {
    pub aggregate_id: String,
    pub seq: i64,
    pub version: i64,
}

/// From reference/packages/schema/src/session-event.ts — the durable event
/// payload union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EventData {
    #[serde(rename = "session.next.agent.switched")]
    AgentSwitched {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        agent: String,
    },
    #[serde(rename = "session.next.model.switched")]
    ModelSwitched {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        model: ModelRef,
    },
    #[serde(rename = "session.next.moved")]
    Moved {
        timestamp: u64,
        session_id: String,
        location: JsonMap,
        #[serde(skip_serializing_if = "Option::is_none")]
        subdirectory: Option<String>,
    },
    #[serde(rename = "session.next.prompted")]
    Prompted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        prompt: Prompt,
        delivery: String,
    },
    #[serde(rename = "session.next.prompt.admitted")]
    PromptAdmitted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        prompt: Prompt,
        delivery: String,
    },
    #[serde(rename = "session.next.context.updated")]
    ContextUpdated {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        text: String,
    },
    #[serde(rename = "session.next.synthetic")]
    Synthetic {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        text: String,
    },
    #[serde(rename = "session.next.shell.started")]
    ShellStarted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        command: String,
    },
    #[serde(rename = "session.next.shell.ended")]
    ShellEnded {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        output: String,
    },
    #[serde(rename = "session.next.step.started")]
    StepStarted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        agent: String,
        model: ModelRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
    },
    #[serde(rename = "session.next.step.ended")]
    StepEnded {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        finish: String,
        cost: f64,
        tokens: AssistantTokens,
        #[serde(skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        files: Option<Vec<String>>,
    },
    #[serde(rename = "session.next.step.failed")]
    StepFailed {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        error: UnknownError,
    },
    #[serde(rename = "session.next.text.started")]
    TextStarted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "textID")]
        text_id: String,
    },
    #[serde(rename = "session.next.text.delta")]
    TextDelta {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "textID")]
        text_id: String,
        delta: String,
    },
    #[serde(rename = "session.next.text.ended")]
    TextEnded {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "textID")]
        text_id: String,
        text: String,
    },
    #[serde(rename = "session.next.reasoning.started")]
    ReasoningStarted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "reasoningID")]
        reasoning_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "session.next.reasoning.delta")]
    ReasoningDelta {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "reasoningID")]
        reasoning_id: String,
        delta: String,
    },
    #[serde(rename = "session.next.reasoning.ended")]
    ReasoningEnded {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "reasoningID")]
        reasoning_id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<JsonMap>,
    },
    #[serde(rename = "session.next.tool.input.started")]
    ToolInputStarted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        name: String,
    },
    #[serde(rename = "session.next.tool.input.delta")]
    ToolInputDelta {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        delta: String,
    },
    #[serde(rename = "session.next.tool.input.ended")]
    ToolInputEnded {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        text: String,
    },
    #[serde(rename = "session.next.tool.called")]
    ToolCalled {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        tool: String,
        input: JsonMap,
        provider: ToolCalledProvider,
    },
    #[serde(rename = "session.next.tool.progress")]
    ToolProgress {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        structured: JsonMap,
        content: Vec<ToolContent>,
    },
    #[serde(rename = "session.next.tool.success")]
    ToolSuccess {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        structured: JsonMap,
        content: Vec<ToolContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_paths: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        provider: ToolCalledProvider,
    },
    #[serde(rename = "session.next.tool.failed")]
    ToolFailed {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: String,
        #[serde(rename = "callID")]
        call_id: String,
        error: UnknownError,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        provider: ToolCalledProvider,
    },
    #[serde(rename = "session.next.retried")]
    Retried {
        timestamp: u64,
        session_id: String,
        attempt: f64,
        error: RetryError,
    },
    #[serde(rename = "session.next.compaction.started")]
    CompactionStarted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        reason: String,
    },
    #[serde(rename = "session.next.compaction.delta")]
    CompactionDelta {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        text: String,
    },
    #[serde(rename = "session.next.compaction.ended")]
    CompactionEnded {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
        reason: String,
        text: String,
        recent: String,
    },
    #[serde(rename = "session.next.revert.staged")]
    RevertStaged {
        timestamp: u64,
        session_id: String,
        revert: JsonMap,
    },
    #[serde(rename = "session.next.revert.cleared")]
    RevertCleared { timestamp: u64, session_id: String },
    #[serde(rename = "session.next.revert.committed")]
    RevertCommitted {
        timestamp: u64,
        session_id: String,
        #[serde(rename = "messageID")]
        message_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCalledProvider {
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<f64>,
    pub is_retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<JsonMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
}
