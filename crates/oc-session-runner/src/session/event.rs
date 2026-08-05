use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::message::{MessageID, UnknownError};
use super::schema::{ModelRef, SessionID};
use crate::llm::{ProviderMetadata, ToolContent, ToolResultValue};

/// `Session.Error.RetryError`
/// /// From reference/packages/schema/src/session-event.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<f64>,
    #[serde(rename = "isRetryable")]
    pub is_retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProviderMetadata>,
}

impl Provider {
    pub fn new(executed: bool, metadata: Option<ProviderMetadata>) -> Self {
        Self { executed, metadata }
    }
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

/// The durable/live `session.next.*` event surface emitted by the runner.
/// Field names and defaults mirror `packages/schema/src/session-event.ts`.
/// /// From reference/packages/schema/src/session-event.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SessionEvent {
    #[serde(rename = "session.next.step.started")]
    #[serde(rename_all = "camelCase")]
    StepStarted {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        agent: String,
        model: ModelRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
    },
    #[serde(rename = "session.next.step.ended")]
    #[serde(rename_all = "camelCase")]
    StepEnded {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        finish: String,
        cost: f64,
        tokens: Tokens,
        #[serde(skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        files: Option<Vec<String>>,
    },
    #[serde(rename = "session.next.step.failed")]
    #[serde(rename_all = "camelCase")]
    StepFailed {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        error: UnknownError,
    },
    #[serde(rename = "session.next.text.started")]
    #[serde(rename_all = "camelCase")]
    TextStarted {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "textID")]
        text_id: String,
    },
    #[serde(rename = "session.next.text.delta")]
    #[serde(rename_all = "camelCase")]
    TextDelta {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "textID")]
        text_id: String,
        delta: String,
    },
    #[serde(rename = "session.next.text.ended")]
    #[serde(rename_all = "camelCase")]
    TextEnded {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "textID")]
        text_id: String,
        text: String,
    },
    #[serde(rename = "session.next.reasoning.started")]
    #[serde(rename_all = "camelCase")]
    ReasoningStarted {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "reasoningID")]
        reasoning_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "session.next.reasoning.delta")]
    #[serde(rename_all = "camelCase")]
    ReasoningDelta {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "reasoningID")]
        reasoning_id: String,
        delta: String,
    },
    #[serde(rename = "session.next.reasoning.ended")]
    #[serde(rename_all = "camelCase")]
    ReasoningEnded {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "reasoningID")]
        reasoning_id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "session.next.tool.input.started")]
    #[serde(rename_all = "camelCase")]
    ToolInputStarted {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "callID")]
        call_id: String,
        name: String,
    },
    #[serde(rename = "session.next.tool.input.delta")]
    #[serde(rename_all = "camelCase")]
    ToolInputDelta {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "callID")]
        call_id: String,
        delta: String,
    },
    #[serde(rename = "session.next.tool.input.ended")]
    #[serde(rename_all = "camelCase")]
    ToolInputEnded {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "callID")]
        call_id: String,
        text: String,
    },
    #[serde(rename = "session.next.tool.called")]
    #[serde(rename_all = "camelCase")]
    ToolCalled {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "callID")]
        call_id: String,
        tool: String,
        input: serde_json::Map<String, Value>,
        provider: Provider,
    },
    #[serde(rename = "session.next.tool.progress")]
    #[serde(rename_all = "camelCase")]
    ToolProgress {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "callID")]
        call_id: String,
        structured: serde_json::Map<String, Value>,
        content: Vec<ToolContent>,
    },
    #[serde(rename = "session.next.tool.success")]
    #[serde(rename_all = "camelCase")]
    ToolSuccess {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "callID")]
        call_id: String,
        structured: serde_json::Map<String, Value>,
        content: Vec<ToolContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_paths: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<ToolResultValue>,
        provider: Provider,
    },
    #[serde(rename = "session.next.tool.failed")]
    #[serde(rename_all = "camelCase")]
    ToolFailed {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "assistantMessageID")]
        assistant_message_id: MessageID,
        #[serde(rename = "callID")]
        call_id: String,
        error: UnknownError,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<ToolResultValue>,
        provider: Provider,
    },
    #[serde(rename = "session.next.retried")]
    #[serde(rename_all = "camelCase")]
    Retried {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        attempt: f64,
        error: RetryError,
    },
    #[serde(rename = "session.next.prompted")]
    #[serde(rename_all = "camelCase")]
    Prompted {
        timestamp: String,
        #[serde(rename = "sessionID")]
        session_id: SessionID,
        #[serde(rename = "messageID")]
        message_id: MessageID,
        prompt: Prompt,
        delivery: String,
    },
}

/// `Prompt` in its serialized form (text + optional files).
/// /// From reference/packages/schema/src/prompt.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<super::message::FileAttachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<super::message::AgentAttachment>>,
}
