//! Event types.
//!
//! The event envelope (`id`, `metadata`, `type`, `durable`, `location`, `data`)
//! is from reference/packages/schema/src/event.ts. `SessionDurableEvent` mirrors
//! `SessionEvent.Durable` (reference/packages/schema/src/session-event.ts) and is
//! streamed by `GET /api/session/:sessionID/event`. `OpenCodeEvent` mirrors the
//! `V2Event` union in reference/packages/protocol/src/groups/event.ts (the server
//! event manifest plus the injected `server.connected` event) and is streamed by
//! `GET /api/event`.

// TODO(integration): promote to oc-schema.
use crate::types::location::LocationRef;
use crate::types::model::ModelRef;
use crate::types::permission::{PermissionReply, PermissionSource};
use crate::types::pty::PtyInfo;
use crate::types::question::{QuestionInfo, QuestionTool};
use crate::types::revert::RevertState;
use crate::types::schema::{DateTimeMillis, Delivery, JsonValue};
use crate::types::session_message::{CompactionReason, SessionUnknownError, Tokens, ToolContent};
use std::collections::HashMap;

/// The `durable` envelope field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableInfo {
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    pub seq: i64,
    pub version: i64,
}

fn req_str(value: &JsonValue, name: &str) -> String {
    value
        .get(name)
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn req_field<T: serde::de::DeserializeOwned>(
    value: &JsonValue,
    name: &str,
) -> Result<T, serde_json::Error> {
    serde_json::from_value(value.get(name).cloned().unwrap_or(JsonValue::Null))
}

fn opt_field<T: serde::de::DeserializeOwned>(
    value: &JsonValue,
    name: &str,
) -> Result<Option<T>, serde_json::Error> {
    match value.get(name) {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map(Some),
        _ => Ok(None),
    }
}

macro_rules! decode_payload {
    ($value:expr, $variant:ident, $data_ty:ty) => {{
        use serde::de::Error as _;
        Ok(OpenCodeEvent::$variant {
            id: req_str(&$value, "id"),
            metadata: opt_field(&$value, "metadata").map_err(D::Error::custom)?,
            durable: opt_field(&$value, "durable").map_err(D::Error::custom)?,
            location: opt_field(&$value, "location").map_err(D::Error::custom)?,
            data: req_field(&$value, "data").map_err(D::Error::custom)?,
        })
    }};
}

// ---------------------------------------------------------------------------
// Session durable events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSwitchedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub agent: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSwitchedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub model: ModelRef,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub location: LocationRef,
    #[serde(default)]
    pub subdirectory: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptEventData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub prompt: crate::types::prompt::Prompt,
    pub delivery: Delivery,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUpdatedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellStartedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellEndedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    pub agent: String,
    pub model: ModelRef,
    #[serde(default)]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepEndedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    pub finish: String,
    pub cost: f64,
    pub tokens: Tokens,
    #[serde(default)]
    pub snapshot: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFailedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    pub error: SessionUnknownError,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStartedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "textID")]
    pub text_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEndedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "textID")]
    pub text_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDeltaData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "textID")]
    pub text_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputStartedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputEndedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputDeltaData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProvider {
    pub executed: bool,
    #[serde(default)]
    pub metadata: Option<HashMap<String, HashMap<String, JsonValue>>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCalledData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub tool: String,
    pub input: HashMap<String, JsonValue>,
    pub provider: ToolProvider,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgressData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub structured: HashMap<String, JsonValue>,
    pub content: Vec<ToolContent>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSuccessData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub structured: HashMap<String, JsonValue>,
    pub content: Vec<ToolContent>,
    #[serde(default)]
    pub output_paths: Option<Vec<String>>,
    #[serde(default)]
    pub result: Option<JsonValue>,
    pub provider: ToolProvider,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFailedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub error: SessionUnknownError,
    #[serde(default)]
    pub result: Option<JsonValue>,
    pub provider: ToolProvider,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningStartedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "reasoningID")]
    pub reasoning_id: String,
    #[serde(default)]
    pub provider_metadata: Option<HashMap<String, HashMap<String, JsonValue>>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEndedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "reasoningID")]
    pub reasoning_id: String,
    pub text: String,
    #[serde(default)]
    pub provider_metadata: Option<HashMap<String, HashMap<String, JsonValue>>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningDeltaData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: String,
    #[serde(rename = "reasoningID")]
    pub reasoning_id: String,
    pub delta: String,
}

/// `SessionEvent.RetryError`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryError {
    pub message: String,
    #[serde(default)]
    pub status_code: Option<f64>,
    pub is_retryable: bool,
    #[serde(default)]
    pub response_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub response_body: Option<String>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetriedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub attempt: f64,
    pub error: RetryError,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionStartedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub reason: CompactionReason,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionDeltaData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionEndedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub reason: CompactionReason,
    pub text: String,
    pub recent: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertStagedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub revert: RevertState,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertClearedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertCommittedData {
    pub timestamp: DateTimeMillis,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
}

/// `SessionEvent.Durable` — the durable session events streamed by
/// `GET /api/session/:sessionID/event` and returned by `session.history`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "type")]
pub enum SessionDurableEvent {
    #[serde(rename = "session.next.agent.switched")]
    AgentSwitched {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: AgentSwitchedData,
    },
    #[serde(rename = "session.next.model.switched")]
    ModelSwitched {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ModelSwitchedData,
    },
    #[serde(rename = "session.next.moved")]
    Moved {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: MovedData,
    },
    #[serde(rename = "session.next.prompted")]
    Prompted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: PromptEventData,
    },
    #[serde(rename = "session.next.prompt.admitted")]
    PromptAdmitted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: PromptEventData,
    },
    #[serde(rename = "session.next.context.updated")]
    ContextUpdated {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ContextUpdatedData,
    },
    #[serde(rename = "session.next.synthetic")]
    Synthetic {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: SyntheticData,
    },
    #[serde(rename = "session.next.shell.started")]
    ShellStarted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ShellStartedData,
    },
    #[serde(rename = "session.next.shell.ended")]
    ShellEnded {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ShellEndedData,
    },
    #[serde(rename = "session.next.step.started")]
    StepStarted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: StepStartedData,
    },
    #[serde(rename = "session.next.step.ended")]
    StepEnded {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: StepEndedData,
    },
    #[serde(rename = "session.next.step.failed")]
    StepFailed {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: StepFailedData,
    },
    #[serde(rename = "session.next.text.started")]
    TextStarted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: TextStartedData,
    },
    #[serde(rename = "session.next.text.ended")]
    TextEnded {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: TextEndedData,
    },
    #[serde(rename = "session.next.tool.input.started")]
    ToolInputStarted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ToolInputStartedData,
    },
    #[serde(rename = "session.next.tool.input.ended")]
    ToolInputEnded {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ToolInputEndedData,
    },
    #[serde(rename = "session.next.tool.called")]
    ToolCalled {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ToolCalledData,
    },
    #[serde(rename = "session.next.tool.progress")]
    ToolProgress {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ToolProgressData,
    },
    #[serde(rename = "session.next.tool.success")]
    ToolSuccess {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ToolSuccessData,
    },
    #[serde(rename = "session.next.tool.failed")]
    ToolFailed {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ToolFailedData,
    },
    #[serde(rename = "session.next.reasoning.started")]
    ReasoningStarted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ReasoningStartedData,
    },
    #[serde(rename = "session.next.reasoning.ended")]
    ReasoningEnded {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: ReasoningEndedData,
    },
    #[serde(rename = "session.next.retried")]
    Retried {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: RetriedData,
    },
    #[serde(rename = "session.next.compaction.started")]
    CompactionStarted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: CompactionStartedData,
    },
    #[serde(rename = "session.next.compaction.ended")]
    CompactionEnded {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: CompactionEndedData,
    },
    #[serde(rename = "session.next.revert.staged")]
    RevertStaged {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: RevertStagedData,
    },
    #[serde(rename = "session.next.revert.cleared")]
    RevertCleared {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: RevertClearedData,
    },
    #[serde(rename = "session.next.revert.committed")]
    RevertCommitted {
        id: String,
        #[serde(default)]
        metadata: Option<HashMap<String, JsonValue>>,
        #[serde(default)]
        durable: Option<DurableInfo>,
        #[serde(default)]
        location: Option<LocationRef>,
        data: RevertCommittedData,
    },
}

impl SessionDurableEvent {
    /// The `type` discriminator of this event.
    pub fn event_type(&self) -> &'static str {
        match self {
            SessionDurableEvent::AgentSwitched { .. } => "session.next.agent.switched",
            SessionDurableEvent::ModelSwitched { .. } => "session.next.model.switched",
            SessionDurableEvent::Moved { .. } => "session.next.moved",
            SessionDurableEvent::Prompted { .. } => "session.next.prompted",
            SessionDurableEvent::PromptAdmitted { .. } => "session.next.prompt.admitted",
            SessionDurableEvent::ContextUpdated { .. } => "session.next.context.updated",
            SessionDurableEvent::Synthetic { .. } => "session.next.synthetic",
            SessionDurableEvent::ShellStarted { .. } => "session.next.shell.started",
            SessionDurableEvent::ShellEnded { .. } => "session.next.shell.ended",
            SessionDurableEvent::StepStarted { .. } => "session.next.step.started",
            SessionDurableEvent::StepEnded { .. } => "session.next.step.ended",
            SessionDurableEvent::StepFailed { .. } => "session.next.step.failed",
            SessionDurableEvent::TextStarted { .. } => "session.next.text.started",
            SessionDurableEvent::TextEnded { .. } => "session.next.text.ended",
            SessionDurableEvent::ToolInputStarted { .. } => "session.next.tool.input.started",
            SessionDurableEvent::ToolInputEnded { .. } => "session.next.tool.input.ended",
            SessionDurableEvent::ToolCalled { .. } => "session.next.tool.called",
            SessionDurableEvent::ToolProgress { .. } => "session.next.tool.progress",
            SessionDurableEvent::ToolSuccess { .. } => "session.next.tool.success",
            SessionDurableEvent::ToolFailed { .. } => "session.next.tool.failed",
            SessionDurableEvent::ReasoningStarted { .. } => "session.next.reasoning.started",
            SessionDurableEvent::ReasoningEnded { .. } => "session.next.reasoning.ended",
            SessionDurableEvent::Retried { .. } => "session.next.retried",
            SessionDurableEvent::CompactionStarted { .. } => "session.next.compaction.started",
            SessionDurableEvent::CompactionEnded { .. } => "session.next.compaction.ended",
            SessionDurableEvent::RevertStaged { .. } => "session.next.revert.staged",
            SessionDurableEvent::RevertCleared { .. } => "session.next.revert.cleared",
            SessionDurableEvent::RevertCommitted { .. } => "session.next.revert.committed",
        }
    }
}

// ---------------------------------------------------------------------------
// Other server events (`GET /api/event`)
// ---------------------------------------------------------------------------

/// `file.watcher.updated` event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileWatcherEvent {
    Add,
    Change,
    Unlink,
}

/// `FileSystemWatcher` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWatcherData {
    pub file: String,
    pub event: FileWatcherEvent,
}

/// `permission.v2.asked` event data (`Permission.Request.fields`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAskedData {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub action: String,
    pub resources: Vec<String>,
    #[serde(default)]
    pub save: Option<Vec<String>>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, JsonValue>>,
    #[serde(default)]
    pub source: Option<PermissionSource>,
}

/// `permission.v2.replied` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRepliedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub reply: PermissionReply,
}

/// `question.v2.asked` event data (`Question.Request.fields`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionAskedData {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub questions: Vec<QuestionInfo>,
    #[serde(default)]
    pub tool: Option<QuestionTool>,
}

/// `question.v2.replied` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRepliedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub answers: Vec<Vec<String>>,
}

/// `question.v2.rejected` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRejectedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
}

/// `Todo` item.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoInfo {
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// `todo.updated` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoUpdatedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub todos: Vec<TodoInfo>,
}

/// The `V2Event` union streamed by `GET /api/event` — the server event manifest
/// plus the injected `server.connected` event.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenCodeEvent {
    SessionNext(Box<SessionDurableEvent>),
    TextDelta {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: TextDeltaData,
    },
    ReasoningDelta {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: ReasoningDeltaData,
    },
    ToolInputDelta {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: ToolInputDeltaData,
    },
    CompactionDelta {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: CompactionDeltaData,
    },
    ServerConnected {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
    },
    SessionCreated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    SessionUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    SessionDeleted {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    MessageUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    MessageRemoved {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    PartUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    PartRemoved {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    Diff {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    SessionError {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: JsonValue,
    },
    ModelsDevRefreshed {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
    },
    IntegrationUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
    },
    IntegrationConnectionUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: crate::types::integration::IntegrationRef,
    },
    CatalogUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
    },
    FileEdited {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: FileEditedData,
    },
    ReferenceUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
    },
    PermissionAsked {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: PermissionAskedData,
    },
    PermissionReplied {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: PermissionRepliedData,
    },
    PluginAdded {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: PluginAddedData,
    },
    ProjectDirectoriesUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: ProjectDirectoriesData,
    },
    FileWatcherUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: FileWatcherData,
    },
    PtyCreated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: PtyEventData,
    },
    PtyUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: PtyEventData,
    },
    PtyExited {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: PtyExitedData,
    },
    PtyDeleted {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: PtyDeletedData,
    },
    QuestionAsked {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: QuestionAskedData,
    },
    QuestionReplied {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: QuestionRepliedData,
    },
    QuestionRejected {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: QuestionRejectedData,
    },
    TodoUpdated {
        id: String,
        metadata: Option<HashMap<String, JsonValue>>,
        durable: Option<DurableInfo>,
        location: Option<LocationRef>,
        data: TodoUpdatedData,
    },
    /// An event type unknown to this client; the raw payload is preserved.
    Raw {
        event_type: String,
        value: JsonValue,
    },
}

impl OpenCodeEvent {
    /// The `type` discriminator of this event.
    pub fn event_type(&self) -> &str {
        match self {
            OpenCodeEvent::SessionNext(event) => event.event_type(),
            OpenCodeEvent::TextDelta { .. } => "session.next.text.delta",
            OpenCodeEvent::ReasoningDelta { .. } => "session.next.reasoning.delta",
            OpenCodeEvent::ToolInputDelta { .. } => "session.next.tool.input.delta",
            OpenCodeEvent::CompactionDelta { .. } => "session.next.compaction.delta",
            OpenCodeEvent::ServerConnected { .. } => "server.connected",
            OpenCodeEvent::SessionCreated { .. } => "session.created",
            OpenCodeEvent::SessionUpdated { .. } => "session.updated",
            OpenCodeEvent::SessionDeleted { .. } => "session.deleted",
            OpenCodeEvent::MessageUpdated { .. } => "message.updated",
            OpenCodeEvent::MessageRemoved { .. } => "message.removed",
            OpenCodeEvent::PartUpdated { .. } => "message.part.updated",
            OpenCodeEvent::PartRemoved { .. } => "message.part.removed",
            OpenCodeEvent::Diff { .. } => "session.diff",
            OpenCodeEvent::SessionError { .. } => "session.error",
            OpenCodeEvent::ModelsDevRefreshed { .. } => "models-dev.refreshed",
            OpenCodeEvent::IntegrationUpdated { .. } => "integration.updated",
            OpenCodeEvent::IntegrationConnectionUpdated { .. } => "integration.connection.updated",
            OpenCodeEvent::CatalogUpdated { .. } => "catalog.updated",
            OpenCodeEvent::FileEdited { .. } => "file.edited",
            OpenCodeEvent::ReferenceUpdated { .. } => "reference.updated",
            OpenCodeEvent::PermissionAsked { .. } => "permission.v2.asked",
            OpenCodeEvent::PermissionReplied { .. } => "permission.v2.replied",
            OpenCodeEvent::PluginAdded { .. } => "plugin.added",
            OpenCodeEvent::ProjectDirectoriesUpdated { .. } => "project.directories.updated",
            OpenCodeEvent::FileWatcherUpdated { .. } => "file.watcher.updated",
            OpenCodeEvent::PtyCreated { .. } => "pty.created",
            OpenCodeEvent::PtyUpdated { .. } => "pty.updated",
            OpenCodeEvent::PtyExited { .. } => "pty.exited",
            OpenCodeEvent::PtyDeleted { .. } => "pty.deleted",
            OpenCodeEvent::QuestionAsked { .. } => "question.v2.asked",
            OpenCodeEvent::QuestionReplied { .. } => "question.v2.replied",
            OpenCodeEvent::QuestionRejected { .. } => "question.v2.rejected",
            OpenCodeEvent::TodoUpdated { .. } => "todo.updated",
            OpenCodeEvent::Raw { event_type, .. } => event_type,
        }
    }
}

impl<'de> serde::Deserialize<'de> for OpenCodeEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = JsonValue::deserialize(deserializer)?;
        let event_type = value
            .get("type")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string();
        use serde::de::Error as _;
        let metadata = |value: &JsonValue| {
            opt_field::<HashMap<String, JsonValue>>(value, "metadata").map_err(D::Error::custom)
        };
        let durable = |value: &JsonValue| {
            opt_field::<DurableInfo>(value, "durable").map_err(D::Error::custom)
        };
        let location = |value: &JsonValue| {
            opt_field::<LocationRef>(value, "location").map_err(D::Error::custom)
        };
        match event_type.as_str() {
            "server.connected" => Ok(OpenCodeEvent::ServerConnected {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
            }),
            "session.next.text.delta" => Ok(OpenCodeEvent::TextDelta {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
                data: req_field(&value, "data").map_err(D::Error::custom)?,
            }),
            "session.next.reasoning.delta" => Ok(OpenCodeEvent::ReasoningDelta {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
                data: req_field(&value, "data").map_err(D::Error::custom)?,
            }),
            "session.next.tool.input.delta" => Ok(OpenCodeEvent::ToolInputDelta {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
                data: req_field(&value, "data").map_err(D::Error::custom)?,
            }),
            "session.next.compaction.delta" => Ok(OpenCodeEvent::CompactionDelta {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
                data: req_field(&value, "data").map_err(D::Error::custom)?,
            }),
            "session.next.agent.switched"
            | "session.next.model.switched"
            | "session.next.moved"
            | "session.next.prompted"
            | "session.next.prompt.admitted"
            | "session.next.context.updated"
            | "session.next.synthetic"
            | "session.next.shell.started"
            | "session.next.shell.ended"
            | "session.next.step.started"
            | "session.next.step.ended"
            | "session.next.step.failed"
            | "session.next.text.started"
            | "session.next.text.ended"
            | "session.next.tool.input.started"
            | "session.next.tool.input.ended"
            | "session.next.tool.called"
            | "session.next.tool.progress"
            | "session.next.tool.success"
            | "session.next.tool.failed"
            | "session.next.reasoning.started"
            | "session.next.reasoning.ended"
            | "session.next.retried"
            | "session.next.compaction.started"
            | "session.next.compaction.ended"
            | "session.next.revert.staged"
            | "session.next.revert.cleared"
            | "session.next.revert.committed" => {
                let event = serde_json::from_value::<SessionDurableEvent>(value)
                    .map_err(D::Error::custom)?;
                Ok(OpenCodeEvent::SessionNext(Box::new(event)))
            }
            "session.created" => decode_payload!(value, SessionCreated, JsonValue),
            "session.updated" => decode_payload!(value, SessionUpdated, JsonValue),
            "session.deleted" => decode_payload!(value, SessionDeleted, JsonValue),
            "message.updated" => decode_payload!(value, MessageUpdated, JsonValue),
            "message.removed" => decode_payload!(value, MessageRemoved, JsonValue),
            "message.part.updated" => decode_payload!(value, PartUpdated, JsonValue),
            "message.part.removed" => decode_payload!(value, PartRemoved, JsonValue),
            "session.diff" => decode_payload!(value, Diff, JsonValue),
            "session.error" => decode_payload!(value, SessionError, JsonValue),
            "models-dev.refreshed" => Ok(OpenCodeEvent::ModelsDevRefreshed {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
            }),
            "integration.updated" => Ok(OpenCodeEvent::IntegrationUpdated {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
            }),
            "integration.connection.updated" => decode_payload!(
                value,
                IntegrationConnectionUpdated,
                crate::types::integration::IntegrationRef
            ),
            "catalog.updated" => Ok(OpenCodeEvent::CatalogUpdated {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
            }),
            "file.edited" => decode_payload!(value, FileEdited, FileEditedData),
            "reference.updated" => Ok(OpenCodeEvent::ReferenceUpdated {
                id: req_str(&value, "id"),
                metadata: metadata(&value)?,
                durable: durable(&value)?,
                location: location(&value)?,
            }),
            "permission.v2.asked" => decode_payload!(value, PermissionAsked, PermissionAskedData),
            "permission.v2.replied" => {
                decode_payload!(value, PermissionReplied, PermissionRepliedData)
            }
            "plugin.added" => decode_payload!(value, PluginAdded, PluginAddedData),
            "project.directories.updated" => {
                decode_payload!(value, ProjectDirectoriesUpdated, ProjectDirectoriesData)
            }
            "file.watcher.updated" => decode_payload!(value, FileWatcherUpdated, FileWatcherData),
            "pty.created" => decode_payload!(value, PtyCreated, PtyEventData),
            "pty.updated" => decode_payload!(value, PtyUpdated, PtyEventData),
            "pty.exited" => decode_payload!(value, PtyExited, PtyExitedData),
            "pty.deleted" => decode_payload!(value, PtyDeleted, PtyDeletedData),
            "question.v2.asked" => decode_payload!(value, QuestionAsked, QuestionAskedData),
            "question.v2.replied" => decode_payload!(value, QuestionReplied, QuestionRepliedData),
            "question.v2.rejected" => {
                decode_payload!(value, QuestionRejected, QuestionRejectedData)
            }
            "todo.updated" => decode_payload!(value, TodoUpdated, TodoUpdatedData),
            _ => Ok(OpenCodeEvent::Raw { event_type, value }),
        }
    }
}

/// `file.edited` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEditedData {
    pub file: String,
}

/// `plugin.added` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAddedData {
    pub id: String,
}

/// `project.directories.updated` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDirectoriesData {
    #[serde(rename = "projectID")]
    pub project_id: String,
}

/// `pty.created`/`pty.updated` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyEventData {
    pub info: PtyInfo,
}

/// `pty.exited` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyExitedData {
    pub id: String,
    pub exit_code: u64,
}

/// `pty.deleted` event data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyDeletedData {
    pub id: String,
}
