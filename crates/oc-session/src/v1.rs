/// From reference/packages/schema/src/v1/session.ts
///
/// Legacy "V1" session message/part data model used by the opencode session
/// service. Field names, ordering, and optionality mirror the effect schemas
/// exactly: optional fields are omitted when absent, unions are tagged by
/// `type` (parts), `role` (messages), `status` (tool states) and `name`
/// (named errors).
use serde::{Deserialize, Serialize};

use crate::schema::{create_message, create_part};
use crate::JsonMap;

pub fn message_id(id: Option<&str>) -> String {
    create_message(id)
}

pub fn part_id(id: Option<&str>) -> String {
    create_part(id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    pub additions: f64,
    pub deletions: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// PermissionV1.Rule from reference/packages/schema/src/v1/permission.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: String,
    pub action: String,
}

pub type Ruleset = Vec<PermissionRule>;

/// Named errors serialize as `{ name, data }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "data")]
pub enum Error {
    #[serde(rename = "MessageOutputLengthError")]
    OutputLengthError,
    #[serde(rename = "ProviderAuthError")]
    AuthError {
        provider_id: String,
        message: String,
    },
    #[serde(rename = "MessageAbortedError")]
    AbortedError { message: String },
    #[serde(rename = "StructuredOutputError")]
    StructuredOutputError { message: String, retries: u64 },
    #[serde(rename = "APIError")]
    ApiError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status_code: Option<u64>,
        is_retryable: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_headers: Option<JsonMap>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_body: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<JsonMap>,
    },
    #[serde(rename = "ContextOverflowError")]
    ContextOverflowError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_body: Option<String>,
    },
    #[serde(rename = "ContentFilterError")]
    ContentFilterError { message: String },
    #[serde(rename = "UnknownError")]
    UnknownError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputFormat {
    Text,
    JsonSchema {
        schema: JsonMap,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_count: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartTime {
    pub start: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePartSourceText {
    pub value: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSource {
    pub text: FilePartSourceText,
    #[serde(rename = "type", default = "file_type")]
    pub type_: String,
    pub path: String,
}

fn file_type() -> String {
    "file".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub line: u64,
    pub character: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSource {
    pub text: FilePartSourceText,
    #[serde(rename = "type", default = "symbol_type")]
    pub type_: String,
    pub path: String,
    pub range: Range,
    pub name: String,
    pub kind: u64,
}

fn symbol_type() -> String {
    "symbol".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSource {
    pub text: FilePartSourceText,
    #[serde(rename = "type", default = "resource_type")]
    pub type_: String,
    pub client_name: String,
    pub uri: String,
}

fn resource_type() -> String {
    "resource".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilePartSource {
    File(FileSource),
    Symbol(SymbolSource),
    Resource(ResourceSource),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPartSource {
    pub value: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartBase {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub hash: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<PartTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    pub time: PartTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<FilePartSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AgentPartSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub auto: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow: Option<bool>,
    #[serde(rename = "tail_start_id", skip_serializing_if = "Option::is_none")]
    pub tail_start_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtaskPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub prompt: String,
    pub description: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub attempt: u64,
    pub error: Error,
    pub time: RetryPartTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPartTime {
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepTokens {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
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
pub struct StepFinishPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    pub cost: f64,
    pub tokens: StepTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatePending {
    #[serde(rename = "status")]
    pub status: String,
    pub input: JsonMap,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateRunning {
    #[serde(rename = "status")]
    pub status: String,
    pub input: JsonMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    pub time: RunningTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningTime {
    pub start: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateCompleted {
    #[serde(rename = "status")]
    pub status: String,
    pub input: JsonMap,
    pub output: String,
    pub title: String,
    pub metadata: JsonMap,
    pub time: CompletedTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<FilePart>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTime {
    pub start: u64,
    pub end: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateError {
    #[serde(rename = "status")]
    pub status: String,
    pub input: JsonMap,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    pub time: CompletedTime,
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
pub struct ToolPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub tool: String,
    pub state: ToolState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Part {
    #[serde(rename = "text")]
    Text(TextPart),
    #[serde(rename = "subtask")]
    Subtask(SubtaskPart),
    #[serde(rename = "reasoning")]
    Reasoning(ReasoningPart),
    #[serde(rename = "file")]
    File(FilePart),
    #[serde(rename = "tool")]
    Tool(ToolPart),
    #[serde(rename = "step-start")]
    StepStart(StepStartPart),
    #[serde(rename = "step-finish")]
    StepFinish(StepFinishPart),
    #[serde(rename = "snapshot")]
    Snapshot(SnapshotPart),
    #[serde(rename = "patch")]
    Patch(PatchPart),
    #[serde(rename = "agent")]
    Agent(AgentPart),
    #[serde(rename = "retry")]
    Retry(RetryPart),
    #[serde(rename = "compaction")]
    Compaction(CompactionPart),
}

impl Part {
    pub fn id(&self) -> &str {
        match self {
            Part::Text(p) => &p.base.id,
            Part::Subtask(p) => &p.base.id,
            Part::Reasoning(p) => &p.base.id,
            Part::File(p) => &p.base.id,
            Part::Tool(p) => &p.base.id,
            Part::StepStart(p) => &p.base.id,
            Part::StepFinish(p) => &p.base.id,
            Part::Snapshot(p) => &p.base.id,
            Part::Patch(p) => &p.base.id,
            Part::Agent(p) => &p.base.id,
            Part::Retry(p) => &p.base.id,
            Part::Compaction(p) => &p.base.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub diffs: Vec<FileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserModel {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTime {
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "role")]
    pub role: String,
    pub time: UserTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<OutputFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<UserSummary>,
    pub agent: String,
    pub model: UserModel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<JsonMap>,
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
pub struct AssistantPath {
    pub cwd: String,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTokens {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: CacheTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assistant {
    pub id: String,
    pub session_id: String,
    #[serde(rename = "role")]
    pub role: String,
    pub time: AssistantTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
    #[serde(rename = "parentID")]
    pub parent_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub mode: String,
    pub agent: String,
    pub path: AssistantPath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<bool>,
    pub cost: f64,
    pub tokens: AssistantTokens,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Info {
    #[serde(rename = "user")]
    User(User),
    #[serde(rename = "assistant")]
    Assistant(Assistant),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WithParts {
    pub info: Info,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub additions: f64,
    pub deletions: f64,
    pub files: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<FileDiff>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: CacheTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionShare {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRevert {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "partID", skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModel {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTime {
    pub created: u64,
    pub updated: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacting: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub slug: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    #[serde(rename = "workspaceID", skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SessionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<SessionTokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<SessionShare>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<SessionModel>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    pub time: SessionTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<Ruleset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert: Option<SessionRevert>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_part_round_trips_with_optional_fields() {
        let part = Part::Text(TextPart {
            base: PartBase {
                id: "prt_abc".into(),
                session_id: "ses_1".into(),
                message_id: "msg_abc".into(),
            },
            type_: "text".into(),
            text: "hello".into(),
            synthetic: Some(true),
            ignored: None,
            time: None,
            metadata: None,
        });
        let value = serde_json::to_value(&part).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "prt_abc",
                "sessionID": "ses_1",
                "messageID": "msg_abc",
                "type": "text",
                "text": "hello",
                "synthetic": true
            })
        );
    }

    #[test]
    fn named_error_uses_adjacent_tag() {
        let err = Error::AbortedError {
            message: "Aborted".into(),
        };
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            json!({ "name": "MessageAbortedError", "data": { "message": "Aborted" } })
        );
    }

    #[test]
    fn assistant_serializes_full_shape() {
        let info = Info::Assistant(Assistant {
            id: "msg1".into(),
            session_id: "ses1".into(),
            role: "assistant".into(),
            time: AssistantTime {
                created: 1000,
                completed: None,
            },
            error: None,
            parent_id: "msg0".into(),
            model_id: "gpt-4o".into(),
            provider_id: "openai".into(),
            mode: "primary".into(),
            agent: "primary".into(),
            path: AssistantPath {
                cwd: "/work".into(),
                root: "/work".into(),
            },
            summary: None,
            cost: 0.0,
            tokens: AssistantTokens {
                total: Some(10.0),
                input: 5.0,
                output: 5.0,
                reasoning: 0.0,
                cache: CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            structured: None,
            variant: None,
            finish: None,
        });
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["role"], "assistant");
        assert_eq!(value["tokens"]["total"], 10.0);
    }
}
