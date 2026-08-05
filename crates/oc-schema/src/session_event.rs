//! From reference/packages/schema/src/session-event.ts

use crate::define_event;
use crate::event::Definition;
use crate::llm::{ProviderMetadata, ToolContent};
use crate::location;
use crate::model;
use crate::prompt;
use crate::revert;
use crate::schema::{DateTimeUtc, Finite, NonNegativeInt, RelativePath};
use crate::session_delivery::Delivery;
use crate::session_id::SessionID;
use crate::session_message::{self, CompactionReason, TokenUsage, UnknownError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub use crate::prompt::FileAttachment;

/// `SessionEvent.Source`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Source {
    pub start: NonNegativeInt,
    pub end: NonNegativeInt,
    pub text: String,
}

/// The `Base` fields shared by every session event payload.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Base {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
}

/// `SessionEvent.RetryError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RetryError {
    pub message: String,
    #[serde(rename = "statusCode", skip_serializing_if = "Option::is_none", default)]
    pub status_code: Option<Finite>,
    pub is_retryable: bool,
    #[serde(rename = "responseHeaders", skip_serializing_if = "Option::is_none", default)]
    pub response_headers: Option<IndexMap<String, String>>,
    #[serde(rename = "responseBody", skip_serializing_if = "Option::is_none", default)]
    pub response_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, String>>,
}

/// The `provider` payload of tool events.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolProvider {
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<ProviderMetadata>,
}

// --- data payloads ---

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentSwitchedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    pub agent: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModelSwitchedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    pub model: model::Ref,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MovedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub location: location::Ref,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subdirectory: Option<RelativePath>,
}

/// Payload for `session.next.prompted` and `session.next.prompt.admitted`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PromptFields {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    pub prompt: prompt::Prompt,
    pub delivery: Delivery,
}

/// Payload for `session.next.context.updated`, `session.next.synthetic`, and
/// `session.next.compaction.delta`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MessageTextData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShellStartedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub command: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShellEndedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub output: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepStartedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    pub agent: String,
    pub model: model::Ref,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepEndedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    pub finish: String,
    pub cost: Finite,
    pub tokens: TokenUsage,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub files: Option<Vec<RelativePath>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepFailedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    pub error: UnknownError,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextStartedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    #[serde(rename = "textID")]
    pub text_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextDeltaData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    #[serde(rename = "textID")]
    pub text_id: String,
    pub delta: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextEndedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    #[serde(rename = "textID")]
    pub text_id: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReasoningStartedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    #[serde(rename = "reasoningID")]
    pub reasoning_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_metadata: Option<ProviderMetadata>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReasoningDeltaData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    #[serde(rename = "reasoningID")]
    pub reasoning_id: String,
    pub delta: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReasoningEndedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    #[serde(rename = "reasoningID")]
    pub reasoning_id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// The `ToolBase` fields shared by tool events.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolBase {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: session_message::ID,
    #[serde(rename = "callID")]
    pub call_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolInputStartedData {
    #[serde(flatten)]
    pub base: ToolBase,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolInputDeltaData {
    #[serde(flatten)]
    pub base: ToolBase,
    pub delta: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolInputEndedData {
    #[serde(flatten)]
    pub base: ToolBase,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolCalledData {
    #[serde(flatten)]
    pub base: ToolBase,
    pub tool: String,
    pub input: IndexMap<String, JsonValue>,
    pub provider: ToolProvider,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolProgressData {
    #[serde(flatten)]
    pub base: ToolBase,
    pub structured: IndexMap<String, JsonValue>,
    pub content: Vec<ToolContent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolSuccessData {
    #[serde(flatten)]
    pub base: ToolBase,
    pub structured: IndexMap<String, JsonValue>,
    pub content: Vec<ToolContent>,
    #[serde(rename = "outputPaths", skip_serializing_if = "Option::is_none", default)]
    pub output_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<JsonValue>,
    pub provider: ToolProvider,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolFailedData {
    #[serde(flatten)]
    pub base: ToolBase,
    pub error: UnknownError,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<JsonValue>,
    pub provider: ToolProvider,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RetriedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub attempt: Finite,
    pub error: RetryError,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CompactionStartedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    pub reason: CompactionReason,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CompactionEndedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
    pub reason: CompactionReason,
    pub text: String,
    pub recent: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RevertStagedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub revert: revert::State,
}

/// Payload for `session.next.revert.cleared` (just the `Base` fields).
pub type RevertClearedData = Base;

/// Payload for `session.next.revert.committed`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RevertCommittedData {
    pub timestamp: DateTimeUtc,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: session_message::ID,
}

// --- events ---

define_event! {
    /// `session.next.agent.switched`.
    pub struct AgentSwitched {
        tag: AgentSwitchedTag,
        r#type: "session.next.agent.switched",
        durable: "sessionID", 1,
        data: AgentSwitchedData,
    }
}

define_event! {
    /// `session.next.model.switched`.
    pub struct ModelSwitched {
        tag: ModelSwitchedTag,
        r#type: "session.next.model.switched",
        durable: "sessionID", 1,
        data: ModelSwitchedData,
    }
}

define_event! {
    /// `session.next.moved`.
    pub struct Moved {
        tag: MovedTag,
        r#type: "session.next.moved",
        durable: "sessionID", 1,
        data: MovedData,
    }
}

define_event! {
    /// `session.next.prompted`.
    pub struct Prompted {
        tag: PromptedTag,
        r#type: "session.next.prompted",
        durable: "sessionID", 1,
        data: PromptFields,
    }
}

define_event! {
    /// `session.next.prompt.admitted`.
    pub struct PromptAdmitted {
        tag: PromptAdmittedTag,
        r#type: "session.next.prompt.admitted",
        durable: "sessionID", 1,
        data: PromptFields,
    }
}

define_event! {
    /// `session.next.context.updated`.
    pub struct ContextUpdated {
        tag: ContextUpdatedTag,
        r#type: "session.next.context.updated",
        durable: "sessionID", 1,
        data: MessageTextData,
    }
}

define_event! {
    /// `session.next.synthetic`.
    pub struct Synthetic {
        tag: SyntheticTag,
        r#type: "session.next.synthetic",
        durable: "sessionID", 1,
        data: MessageTextData,
    }
}

define_event! {
    /// `session.next.shell.started`.
    pub struct ShellStarted {
        tag: ShellStartedTag,
        r#type: "session.next.shell.started",
        durable: "sessionID", 1,
        data: ShellStartedData,
    }
}

define_event! {
    /// `session.next.shell.ended`.
    pub struct ShellEnded {
        tag: ShellEndedTag,
        r#type: "session.next.shell.ended",
        durable: "sessionID", 1,
        data: ShellEndedData,
    }
}

define_event! {
    /// `session.next.step.started`.
    pub struct StepStarted {
        tag: StepStartedTag,
        r#type: "session.next.step.started",
        durable: "sessionID", 1,
        data: StepStartedData,
    }
}

define_event! {
    /// `session.next.step.ended`.
    pub struct StepEnded {
        tag: StepEndedTag,
        r#type: "session.next.step.ended",
        durable: "sessionID", 2,
        data: StepEndedData,
    }
}

define_event! {
    /// `session.next.step.failed`.
    pub struct StepFailed {
        tag: StepFailedTag,
        r#type: "session.next.step.failed",
        durable: "sessionID", 2,
        data: StepFailedData,
    }
}

define_event! {
    /// `session.next.text.started`.
    pub struct TextStarted {
        tag: TextStartedTag,
        r#type: "session.next.text.started",
        durable: "sessionID", 1,
        data: TextStartedData,
    }
}

define_event! {
    /// `session.next.text.delta` — live-only.
    pub struct TextDelta {
        tag: TextDeltaTag,
        r#type: "session.next.text.delta",
        data: TextDeltaData,
    }
}

define_event! {
    /// `session.next.text.ended`.
    pub struct TextEnded {
        tag: TextEndedTag,
        r#type: "session.next.text.ended",
        durable: "sessionID", 1,
        data: TextEndedData,
    }
}

define_event! {
    /// `session.next.reasoning.started`.
    pub struct ReasoningStarted {
        tag: ReasoningStartedTag,
        r#type: "session.next.reasoning.started",
        durable: "sessionID", 1,
        data: ReasoningStartedData,
    }
}

define_event! {
    /// `session.next.reasoning.delta` — live-only.
    pub struct ReasoningDelta {
        tag: ReasoningDeltaTag,
        r#type: "session.next.reasoning.delta",
        data: ReasoningDeltaData,
    }
}

define_event! {
    /// `session.next.reasoning.ended`.
    pub struct ReasoningEnded {
        tag: ReasoningEndedTag,
        r#type: "session.next.reasoning.ended",
        durable: "sessionID", 1,
        data: ReasoningEndedData,
    }
}

define_event! {
    /// `session.next.tool.input.started`.
    pub struct ToolInputStarted {
        tag: ToolInputStartedTag,
        r#type: "session.next.tool.input.started",
        durable: "sessionID", 1,
        data: ToolInputStartedData,
    }
}

define_event! {
    /// `session.next.tool.input.delta` — live-only.
    pub struct ToolInputDelta {
        tag: ToolInputDeltaTag,
        r#type: "session.next.tool.input.delta",
        data: ToolInputDeltaData,
    }
}

define_event! {
    /// `session.next.tool.input.ended`.
    pub struct ToolInputEnded {
        tag: ToolInputEndedTag,
        r#type: "session.next.tool.input.ended",
        durable: "sessionID", 1,
        data: ToolInputEndedData,
    }
}

define_event! {
    /// `session.next.tool.called`.
    pub struct ToolCalled {
        tag: ToolCalledTag,
        r#type: "session.next.tool.called",
        durable: "sessionID", 1,
        data: ToolCalledData,
    }
}

define_event! {
    /// `session.next.tool.progress`.
    pub struct ToolProgress {
        tag: ToolProgressTag,
        r#type: "session.next.tool.progress",
        durable: "sessionID", 1,
        data: ToolProgressData,
    }
}

define_event! {
    /// `session.next.tool.success`.
    pub struct ToolSuccess {
        tag: ToolSuccessTag,
        r#type: "session.next.tool.success",
        durable: "sessionID", 1,
        data: ToolSuccessData,
    }
}

define_event! {
    /// `session.next.tool.failed`.
    pub struct ToolFailed {
        tag: ToolFailedTag,
        r#type: "session.next.tool.failed",
        durable: "sessionID", 1,
        data: ToolFailedData,
    }
}

define_event! {
    /// `session.next.retried`.
    pub struct Retried {
        tag: RetriedTag,
        r#type: "session.next.retried",
        durable: "sessionID", 1,
        data: RetriedData,
    }
}

define_event! {
    /// `session.next.compaction.started`.
    pub struct CompactionStarted {
        tag: CompactionStartedTag,
        r#type: "session.next.compaction.started",
        durable: "sessionID", 1,
        data: CompactionStartedData,
    }
}

define_event! {
    /// `session.next.compaction.delta` — live-only.
    pub struct CompactionDelta {
        tag: CompactionDeltaTag,
        r#type: "session.next.compaction.delta",
        data: MessageTextData,
    }
}

define_event! {
    /// `session.next.compaction.ended`.
    pub struct CompactionEnded {
        tag: CompactionEndedTag,
        r#type: "session.next.compaction.ended",
        durable: "sessionID", 1,
        data: CompactionEndedData,
    }
}

define_event! {
    /// `session.next.revert.staged`.
    pub struct RevertStaged {
        tag: RevertStagedTag,
        r#type: "session.next.revert.staged",
        durable: "sessionID", 1,
        data: RevertStagedData,
    }
}

define_event! {
    /// `session.next.revert.cleared`.
    pub struct RevertCleared {
        tag: RevertClearedTag,
        r#type: "session.next.revert.cleared",
        durable: "sessionID", 1,
        data: RevertClearedData,
    }
}

define_event! {
    /// `session.next.revert.committed`.
    pub struct RevertCommitted {
        tag: RevertCommittedTag,
        r#type: "session.next.revert.committed",
        durable: "sessionID", 1,
        data: RevertCommittedData,
    }
}

/// `SessionEvent.DurableDefinitions`.
pub const DURABLE_DEFINITIONS: &[Definition] = &[
    AgentSwitched::definition(),
    ModelSwitched::definition(),
    Moved::definition(),
    Prompted::definition(),
    PromptAdmitted::definition(),
    ContextUpdated::definition(),
    Synthetic::definition(),
    ShellStarted::definition(),
    ShellEnded::definition(),
    StepStarted::definition(),
    StepEnded::definition(),
    StepFailed::definition(),
    TextStarted::definition(),
    TextEnded::definition(),
    ReasoningStarted::definition(),
    ReasoningEnded::definition(),
    ToolInputStarted::definition(),
    ToolInputEnded::definition(),
    ToolCalled::definition(),
    ToolProgress::definition(),
    ToolSuccess::definition(),
    ToolFailed::definition(),
    Retried::definition(),
    CompactionStarted::definition(),
    CompactionEnded::definition(),
    RevertStaged::definition(),
    RevertCleared::definition(),
    RevertCommitted::definition(),
];

/// `SessionEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    AgentSwitched::definition(),
    ModelSwitched::definition(),
    Moved::definition(),
    Prompted::definition(),
    PromptAdmitted::definition(),
    ContextUpdated::definition(),
    Synthetic::definition(),
    ShellStarted::definition(),
    ShellEnded::definition(),
    StepStarted::definition(),
    StepEnded::definition(),
    StepFailed::definition(),
    TextStarted::definition(),
    TextDelta::definition(),
    TextEnded::definition(),
    ReasoningStarted::definition(),
    ReasoningDelta::definition(),
    ReasoningEnded::definition(),
    ToolInputStarted::definition(),
    ToolInputDelta::definition(),
    ToolInputEnded::definition(),
    ToolCalled::definition(),
    ToolProgress::definition(),
    ToolSuccess::definition(),
    ToolFailed::definition(),
    Retried::definition(),
    CompactionStarted::definition(),
    CompactionDelta::definition(),
    CompactionEnded::definition(),
    RevertStaged::definition(),
    RevertCleared::definition(),
    RevertCommitted::definition(),
];

/// `SessionEvent.Durable` — tagged union of the durable session events.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum DurableEvent {
    AgentSwitched(AgentSwitched),
    ModelSwitched(ModelSwitched),
    Moved(Moved),
    Prompted(Prompted),
    PromptAdmitted(PromptAdmitted),
    ContextUpdated(ContextUpdated),
    Synthetic(Synthetic),
    ShellStarted(ShellStarted),
    ShellEnded(ShellEnded),
    StepStarted(StepStarted),
    StepEnded(StepEnded),
    StepFailed(StepFailed),
    TextStarted(TextStarted),
    TextEnded(TextEnded),
    ReasoningStarted(ReasoningStarted),
    ReasoningEnded(ReasoningEnded),
    ToolInputStarted(ToolInputStarted),
    ToolInputEnded(ToolInputEnded),
    ToolCalled(ToolCalled),
    ToolProgress(ToolProgress),
    ToolSuccess(ToolSuccess),
    ToolFailed(ToolFailed),
    Retried(Retried),
    CompactionStarted(CompactionStarted),
    CompactionEnded(CompactionEnded),
    RevertStaged(RevertStaged),
    RevertCleared(RevertCleared),
    RevertCommitted(RevertCommitted),
}

/// `SessionEvent.All` — tagged union of every session event.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Event {
    AgentSwitched(AgentSwitched),
    ModelSwitched(ModelSwitched),
    Moved(Moved),
    Prompted(Prompted),
    PromptAdmitted(PromptAdmitted),
    ContextUpdated(ContextUpdated),
    Synthetic(Synthetic),
    ShellStarted(ShellStarted),
    ShellEnded(ShellEnded),
    StepStarted(StepStarted),
    StepEnded(StepEnded),
    StepFailed(StepFailed),
    TextStarted(TextStarted),
    TextDelta(TextDelta),
    TextEnded(TextEnded),
    ReasoningStarted(ReasoningStarted),
    ReasoningDelta(ReasoningDelta),
    ReasoningEnded(ReasoningEnded),
    ToolInputStarted(ToolInputStarted),
    ToolInputDelta(ToolInputDelta),
    ToolInputEnded(ToolInputEnded),
    ToolCalled(ToolCalled),
    ToolProgress(ToolProgress),
    ToolSuccess(ToolSuccess),
    ToolFailed(ToolFailed),
    Retried(Retried),
    CompactionStarted(CompactionStarted),
    CompactionDelta(CompactionDelta),
    CompactionEnded(CompactionEnded),
    RevertStaged(RevertStaged),
    RevertCleared(RevertCleared),
    RevertCommitted(RevertCommitted),
}

/// `SessionEvent.Type`.
pub type Type = String;
