/// From reference/packages/opencode/src/session/message.ts
///
/// The opencode session `Message` model (AI SDK-shaped messages stored in the
/// message table) plus `MessageError` shared named errors.
use serde::{Deserialize, Serialize};

use crate::JsonMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum ToolInvocation {
    #[serde(rename = "call", rename_all = "camelCase")]
    Call {
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<u64>,
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
    },
    #[serde(rename = "partial-call", rename_all = "camelCase")]
    PartialCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<u64>,
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
    },
    #[serde(rename = "result", rename_all = "camelCase")]
    Result {
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<u64>,
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
        result: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocationPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub tool_invocation: ToolInvocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceUrlPart {
    #[serde(rename = "type")]
    pub type_: String,
    pub source_id: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    #[serde(rename = "type")]
    pub type_: String,
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartPart {
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessagePart {
    #[serde(rename = "text")]
    Text(TextPart),
    #[serde(rename = "reasoning")]
    Reasoning(ReasoningPart),
    #[serde(rename = "tool-invocation")]
    ToolInvocation(ToolInvocationPart),
    #[serde(rename = "source-url")]
    SourceUrl(SourceUrlPart),
    #[serde(rename = "file")]
    File(FilePart),
    #[serde(rename = "step-start")]
    StepStart(StepStartPart),
}

/// MessageError.SharedSchema from reference message-error.ts — named errors
/// serialized as `{ name, data }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "data")]
pub enum MessageError {
    #[serde(rename = "ProviderAuthError")]
    AuthError {
        provider_id: String,
        message: String,
    },
    #[serde(rename = "UnknownError")]
    UnknownError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
    },
    #[serde(rename = "MessageOutputLengthError")]
    OutputLengthError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataTime {
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTime {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMetadata {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    pub time: ToolTime,
    #[serde(flatten)]
    pub rest: JsonMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantPath {
    pub cwd: String,
    pub root: String,
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
pub struct AssistantMetadata {
    pub system: Vec<String>,
    pub model_id: String,
    pub provider_id: String,
    pub path: AssistantPath,
    pub cost: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<bool>,
    pub tokens: AssistantTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub time: MetadataTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,
    pub session_id: String,
    pub tool: JsonMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant: Option<AssistantMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: String,
    pub role: String,
    pub parts: Vec<MessagePart>,
    pub metadata: Metadata,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_invocation_is_state_tagged() {
        let call = ToolInvocation::Call {
            step: None,
            tool_call_id: "call_1".into(),
            tool_name: "bash".into(),
            args: json!({ "cmd": "ls" }),
        };
        let value = serde_json::to_value(&call).unwrap();
        assert_eq!(value["state"], "call");
        assert_eq!(value["toolCallId"], "call_1");
    }
}
