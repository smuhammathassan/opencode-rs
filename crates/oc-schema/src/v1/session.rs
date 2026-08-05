//! From reference/packages/schema/src/v1/session.ts

use crate::define_event;
use crate::file_diff;
use crate::identifier::ascending;
use crate::model;
use crate::project;
use crate::provider;
use crate::schema::{Finite, NonNegativeInt};
use crate::session_id::SessionID;
use crate::workspace_id::WorkspaceID;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// `SessionV1.MessageID` — starts with `msg`.
pub type MessageID = String;

/// `SessionV1.PartID` — starts with `prt`.
pub type PartID = String;

/// `MessageID.ascending(id?)`.
pub fn message_id(id: Option<String>) -> MessageID {
    match id {
        Some(id) => id,
        None => format!("msg_{}", ascending()),
    }
}

/// `PartID.ascending(id?)`.
pub fn part_id(id: Option<String>) -> PartID {
    match id {
        Some(id) => id,
        None => format!("prt_{}", ascending()),
    }
}

// --- named errors ---

/// `MessageOutputLengthError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutputLengthError {
    #[serde(rename = "name")]
    pub name: OutputLengthErrorName,
    pub data: OutputLengthErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum OutputLengthErrorName {
    #[serde(rename = "MessageOutputLengthError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutputLengthErrorData {}

/// `ProviderAuthError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AuthError {
    #[serde(rename = "name")]
    pub name: AuthErrorName,
    pub data: AuthErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AuthErrorName {
    #[serde(rename = "ProviderAuthError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AuthErrorData {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub message: String,
}

/// `MessageAbortedError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AbortedError {
    #[serde(rename = "name")]
    pub name: AbortedErrorName,
    pub data: AbortedErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AbortedErrorName {
    #[serde(rename = "MessageAbortedError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AbortedErrorData {
    pub message: String,
}

/// `StructuredOutputError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StructuredOutputError {
    #[serde(rename = "name")]
    pub name: StructuredOutputErrorName,
    pub data: StructuredOutputErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum StructuredOutputErrorName {
    #[serde(rename = "StructuredOutputError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StructuredOutputErrorData {
    pub message: String,
    pub retries: NonNegativeInt,
}

/// `APIError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct APIError {
    #[serde(rename = "name")]
    pub name: APIErrorName,
    pub data: APIErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum APIErrorName {
    #[serde(rename = "APIError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct APIErrorData {
    pub message: String,
    #[serde(
        rename = "statusCode",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub status_code: Option<NonNegativeInt>,
    #[serde(rename = "isRetryable")]
    pub is_retryable: bool,
    #[serde(
        rename = "responseHeaders",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub response_headers: Option<IndexMap<String, String>>,
    #[serde(
        rename = "responseBody",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub response_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, String>>,
}

/// `ContextOverflowError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContextOverflowError {
    #[serde(rename = "name")]
    pub name: ContextOverflowErrorName,
    pub data: ContextOverflowErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ContextOverflowErrorName {
    #[serde(rename = "ContextOverflowError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContextOverflowErrorData {
    pub message: String,
    #[serde(
        rename = "responseBody",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub response_body: Option<String>,
}

/// `ContentFilterError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContentFilterError {
    #[serde(rename = "name")]
    pub name: ContentFilterErrorName,
    pub data: ContentFilterErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ContentFilterErrorName {
    #[serde(rename = "ContentFilterError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContentFilterErrorData {
    pub message: String,
}

/// The `AssistantError` union discriminated on `name`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum AssistantError {
    Auth(AuthError),
    Unknown(UnknownError),
    OutputLength(OutputLengthError),
    Aborted(AbortedError),
    StructuredOutput(StructuredOutputError),
    ContextOverflow(ContextOverflowError),
    ContentFilter(ContentFilterError),
    API(APIError),
}

/// `UnknownError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UnknownError {
    #[serde(rename = "name")]
    pub name: UnknownErrorName,
    pub data: UnknownErrorData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum UnknownErrorName {
    #[serde(rename = "UnknownError")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UnknownErrorData {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub r#ref: Option<String>,
}

// --- output format ---

/// `OutputFormatText`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutputFormatText {
    #[serde(rename = "type")]
    pub r#type: OutputFormatTextType,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum OutputFormatTextType {
    #[serde(rename = "text")]
    Value,
}

fn default_retry_count() -> Option<NonNegativeInt> {
    Some(2)
}

/// `OutputFormatJsonSchema`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutputFormatJsonSchema {
    #[serde(rename = "type")]
    pub r#type: OutputFormatJsonSchemaType,
    pub schema: IndexMap<String, JsonValue>,
    #[serde(rename = "retryCount", default = "default_retry_count")]
    pub retry_count: Option<NonNegativeInt>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum OutputFormatJsonSchemaType {
    #[serde(rename = "json_schema")]
    Value,
}

/// `OutputFormat` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Format {
    Text(OutputFormatText),
    JsonSchema(OutputFormatJsonSchema),
}

// --- parts ---

/// `Range`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// `Range` position.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Position {
    pub line: NonNegativeInt,
    pub character: NonNegativeInt,
}

/// `FilePartSource.text`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FilePartSourceText {
    pub value: String,
    pub start: Finite,
    pub end: Finite,
}

/// `FileSource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileSource {
    pub text: FilePartSourceText,
    #[serde(rename = "type")]
    pub r#type: FileSourceType,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum FileSourceType {
    #[serde(rename = "file")]
    Value,
}

/// `SymbolSource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SymbolSource {
    pub text: FilePartSourceText,
    #[serde(rename = "type")]
    pub r#type: SymbolSourceType,
    pub path: String,
    pub range: Range,
    pub name: String,
    pub kind: NonNegativeInt,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SymbolSourceType {
    #[serde(rename = "symbol")]
    Value,
}

/// `ResourceSource`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ResourceSource {
    pub text: FilePartSourceText,
    #[serde(rename = "type")]
    pub r#type: ResourceSourceType,
    #[serde(rename = "clientName")]
    pub client_name: String,
    pub uri: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ResourceSourceType {
    #[serde(rename = "resource")]
    Value,
}

/// `FilePartSource` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum FilePartSource {
    File(FileSource),
    Symbol(SymbolSource),
    Resource(ResourceSource),
}

/// `partBase` shared by all parts.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PartBase {
    pub id: PartID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: MessageID,
}

/// `SnapshotPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SnapshotPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: SnapshotPartType,
    pub snapshot: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SnapshotPartType {
    #[serde(rename = "snapshot")]
    Value,
}

/// `PatchPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PatchPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: PatchPartType,
    pub hash: String,
    pub files: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum PatchPartType {
    #[serde(rename = "patch")]
    Value,
}

/// `TextPart.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextPartTime {
    pub start: NonNegativeInt,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub end: Option<NonNegativeInt>,
}

/// `TextPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: TextPartType,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub synthetic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ignored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub time: Option<TextPartTime>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TextPartType {
    #[serde(rename = "text")]
    Value,
}

/// `ReasoningPart.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReasoningPartTime {
    pub start: NonNegativeInt,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub end: Option<NonNegativeInt>,
}

/// `ReasoningPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReasoningPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: ReasoningPartType,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
    pub time: ReasoningPartTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ReasoningPartType {
    #[serde(rename = "reasoning")]
    Value,
}

/// `FilePart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FilePart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: FilePartType,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub filename: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<FilePartSource>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum FilePartType {
    #[serde(rename = "file")]
    Value,
}

/// `AgentPart.source`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentPartSource {
    pub value: String,
    pub start: NonNegativeInt,
    pub end: NonNegativeInt,
}

/// `AgentPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: AgentPartType,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<AgentPartSource>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AgentPartType {
    #[serde(rename = "agent")]
    Value,
}

/// `CompactionPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CompactionPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: CompactionPartType,
    pub auto: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub overflow: Option<bool>,
    #[serde(
        rename = "tail_start_id",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub tail_start_id: Option<MessageID>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CompactionPartType {
    #[serde(rename = "compaction")]
    Value,
}

/// `SubtaskPart.model`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SubtaskPartModel {
    #[serde(rename = "providerID")]
    pub provider_id: provider::ID,
    #[serde(rename = "modelID")]
    pub model_id: model::ID,
}

/// `SubtaskPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SubtaskPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: SubtaskPartType,
    pub prompt: String,
    pub description: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<SubtaskPartModel>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub command: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SubtaskPartType {
    #[serde(rename = "subtask")]
    Value,
}

/// `RetryPart.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RetryPartTime {
    pub created: NonNegativeInt,
}

/// `RetryPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RetryPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: RetryPartType,
    pub attempt: NonNegativeInt,
    pub error: APIError,
    pub time: RetryPartTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum RetryPartType {
    #[serde(rename = "retry")]
    Value,
}

/// `StepStartPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepStartPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: StepStartPartType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum StepStartPartType {
    #[serde(rename = "step-start")]
    Value,
}

/// `StepFinishPart.tokens`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepFinishTokens {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total: Option<Finite>,
    pub input: Finite,
    pub output: Finite,
    pub reasoning: Finite,
    pub cache: StepFinishCache,
}

/// `StepFinishPart.tokens.cache`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepFinishCache {
    pub read: Finite,
    pub write: Finite,
}

/// `StepFinishPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepFinishPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: StepFinishPartType,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<String>,
    pub cost: Finite,
    pub tokens: StepFinishTokens,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum StepFinishPartType {
    #[serde(rename = "step-finish")]
    Value,
}

/// `ToolStatePending`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStatePending {
    pub status: ToolStatePendingStatus,
    pub input: IndexMap<String, JsonValue>,
    pub raw: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStatePendingStatus {
    #[serde(rename = "pending")]
    Value,
}

/// `ToolStateRunning.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateRunningTime {
    pub start: NonNegativeInt,
}

/// `ToolStateRunning`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateRunning {
    pub status: ToolStateRunningStatus,
    pub input: IndexMap<String, JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
    pub time: ToolStateRunningTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStateRunningStatus {
    #[serde(rename = "running")]
    Value,
}

/// `ToolStateCompleted.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateCompletedTime {
    pub start: NonNegativeInt,
    pub end: NonNegativeInt,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub compacted: Option<NonNegativeInt>,
}

/// `ToolStateCompleted`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateCompleted {
    pub status: ToolStateCompletedStatus,
    pub input: IndexMap<String, JsonValue>,
    pub output: String,
    pub title: String,
    pub metadata: IndexMap<String, JsonValue>,
    pub time: ToolStateCompletedTime,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub attachments: Option<Vec<FilePart>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStateCompletedStatus {
    #[serde(rename = "completed")]
    Value,
}

/// `ToolStateError.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateErrorTime {
    pub start: NonNegativeInt,
    pub end: NonNegativeInt,
}

/// `ToolStateError`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolStateError {
    pub status: ToolStateErrorStatus,
    pub input: IndexMap<String, JsonValue>,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
    pub time: ToolStateErrorTime,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolStateErrorStatus {
    #[serde(rename = "error")]
    Value,
}

/// `ToolState` — tagged union on `status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ToolState {
    Pending(ToolStatePending),
    Running(ToolStateRunning),
    Completed(ToolStateCompleted),
    Error(ToolStateError),
}

/// `ToolPart`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(rename = "type")]
    pub r#type: ToolPartType,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub tool: String,
    pub state: ToolState,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToolPartType {
    #[serde(rename = "tool")]
    Value,
}

/// `Part` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Part {
    Text(TextPart),
    Subtask(SubtaskPart),
    Reasoning(ReasoningPart),
    File(FilePart),
    Tool(ToolPart),
    StepStart(StepStartPart),
    StepFinish(StepFinishPart),
    Snapshot(SnapshotPart),
    Patch(PatchPart),
    Agent(AgentPart),
    Retry(RetryPart),
    Compaction(CompactionPart),
}

// --- messages ---

/// `UserMessage.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UserTime {
    pub created: Finite,
}

/// `UserMessage.summary`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UserSummary {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub body: Option<String>,
    pub diffs: Vec<file_diff::Info>,
}

/// `UserMessage.model`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UserModel {
    #[serde(rename = "providerID")]
    pub provider_id: provider::ID,
    #[serde(rename = "modelID")]
    pub model_id: model::ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub variant: Option<String>,
}

/// `UserMessage`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct User {
    pub id: MessageID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub role: UserRole,
    pub time: UserTime,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub format: Option<Format>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<UserSummary>,
    pub agent: String,
    pub model: UserModel,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tools: Option<IndexMap<String, bool>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum UserRole {
    #[serde(rename = "user")]
    Value,
}

/// `AssistantMessage.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantTime {
    pub created: Finite,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub completed: Option<Finite>,
}

/// `AssistantMessage.path`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantPath {
    pub cwd: String,
    pub root: String,
}

/// `AssistantMessage.tokens`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantTokens {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total: Option<Finite>,
    pub input: Finite,
    pub output: Finite,
    pub reasoning: Finite,
    pub cache: AssistantCache,
}

/// `AssistantMessage.tokens.cache`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantCache {
    pub read: Finite,
    pub write: Finite,
}

/// `AssistantMessage`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Assistant {
    pub id: MessageID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub role: AssistantRole,
    pub time: AssistantTime,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<AssistantError>,
    #[serde(rename = "parentID")]
    pub parent_id: MessageID,
    #[serde(rename = "modelID")]
    pub model_id: model::ID,
    #[serde(rename = "providerID")]
    pub provider_id: provider::ID,
    pub mode: String,
    pub agent: String,
    pub path: AssistantPath,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<bool>,
    pub cost: Finite,
    pub tokens: AssistantTokens,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structured: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub finish: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AssistantRole {
    #[serde(rename = "assistant")]
    Value,
}

/// `Message` (`SessionV1.Info`) — tagged union on `role`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Info {
    User(User),
    Assistant(Assistant),
}

/// `SessionV1.WithParts`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WithParts {
    pub info: Info,
    pub parts: Vec<Part>,
}

/// `TextPartInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextPartInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<PartID>,
    #[serde(rename = "type")]
    pub r#type: TextPartInputType,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub synthetic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ignored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub time: Option<TextPartTime>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TextPartInputType {
    #[serde(rename = "text")]
    Value,
}

/// `FilePartInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FilePartInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<PartID>,
    #[serde(rename = "type")]
    pub r#type: FilePartInputType,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub filename: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<FilePartSource>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum FilePartInputType {
    #[serde(rename = "file")]
    Value,
}

/// `AgentPartInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentPartInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<PartID>,
    #[serde(rename = "type")]
    pub r#type: AgentPartInputType,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<AgentPartSource>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AgentPartInputType {
    #[serde(rename = "agent")]
    Value,
}

/// `SubtaskPartInput`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SubtaskPartInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<PartID>,
    #[serde(rename = "type")]
    pub r#type: SubtaskPartInputType,
    pub prompt: String,
    pub description: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<SubtaskPartModel>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub command: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SubtaskPartInputType {
    #[serde(rename = "subtask")]
    Value,
}

/// `SessionV1.SessionSummary`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub additions: Finite,
    pub deletions: Finite,
    pub files: Finite,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diffs: Option<Vec<file_diff::Info>>,
}

/// `SessionV1.SessionTokens`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionTokens {
    pub input: Finite,
    pub output: Finite,
    pub reasoning: Finite,
    pub cache: SessionCache,
}

/// `SessionV1.SessionTokens.cache`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionCache {
    pub read: Finite,
    pub write: Finite,
}

/// `SessionV1.SessionShare`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionShare {
    pub url: String,
}

/// `SessionV1.SessionRevert`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionRevert {
    #[serde(rename = "messageID")]
    pub message_id: MessageID,
    #[serde(rename = "partID", skip_serializing_if = "Option::is_none", default)]
    pub part_id: Option<PartID>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diff: Option<String>,
}

/// `SessionV1.SessionModel`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionModel {
    pub id: model::ID,
    #[serde(rename = "providerID")]
    pub provider_id: provider::ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub variant: Option<String>,
}

/// `SessionV1.SessionInfo.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionInfoTime {
    pub created: NonNegativeInt,
    pub updated: NonNegativeInt,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub compacting: Option<NonNegativeInt>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub archived: Option<Finite>,
}

/// `SessionV1.SessionInfo`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionInfo {
    pub id: SessionID,
    pub slug: String,
    #[serde(rename = "projectID")]
    pub project_id: project::ID,
    #[serde(
        rename = "workspaceID",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub workspace_id: Option<WorkspaceID>,
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none", default)]
    pub parent_id: Option<SessionID>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<SessionSummary>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cost: Option<Finite>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tokens: Option<SessionTokens>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub share: Option<SessionShare>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<SessionModel>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<IndexMap<String, JsonValue>>,
    pub time: SessionInfoTime,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub permission: Option<crate::v1::permission::Ruleset>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub revert: Option<SessionRevert>,
}

// --- events ---

/// Payload shared by `session.created` / `session.updated` / `session.deleted`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionEventData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub info: SessionInfo,
}

define_event! {
    /// `session.created`.
    pub struct Created {
        tag: CreatedTag,
        r#type: "session.created",
        durable: "sessionID", 1,
        data: SessionEventData,
    }
}

define_event! {
    /// `session.updated`.
    pub struct Updated {
        tag: UpdatedTag,
        r#type: "session.updated",
        durable: "sessionID", 1,
        data: SessionEventData,
    }
}

define_event! {
    /// `session.deleted`.
    pub struct Deleted {
        tag: DeletedTag,
        r#type: "session.deleted",
        durable: "sessionID", 1,
        data: SessionEventData,
    }
}

define_event! {
    /// `message.updated`.
    pub struct MessageUpdated {
        tag: MessageUpdatedTag,
        r#type: "message.updated",
        durable: "sessionID", 1,
        data: MessageUpdatedData,
    }
}

define_event! {
    /// `message.removed`.
    pub struct MessageRemoved {
        tag: MessageRemovedTag,
        r#type: "message.removed",
        durable: "sessionID", 1,
        data: MessageRemovedData,
    }
}

define_event! {
    /// `message.part.updated`.
    pub struct PartUpdated {
        tag: PartUpdatedTag,
        r#type: "message.part.updated",
        durable: "sessionID", 1,
        data: PartUpdatedData,
    }
}

define_event! {
    /// `message.part.removed`.
    pub struct PartRemoved {
        tag: PartRemovedTag,
        r#type: "message.part.removed",
        durable: "sessionID", 1,
        data: PartRemovedData,
    }
}

define_event! {
    /// `message.part.delta`.
    pub struct PartDelta {
        tag: PartDeltaTag,
        r#type: "message.part.delta",
        data: PartDeltaData,
    }
}

define_event! {
    /// `session.diff`.
    pub struct Diff {
        tag: DiffTag,
        r#type: "session.diff",
        data: DiffData,
    }
}

define_event! {
    /// `session.error`.
    pub struct ErrorEvent {
        tag: ErrorEventTag,
        r#type: "session.error",
        data: ErrorEventData,
    }
}

/// Payload of `message.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MessageUpdatedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub info: Info,
}

/// Payload of `message.removed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MessageRemovedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: MessageID,
}

/// Payload of `message.part.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct PartUpdatedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub part: Part,
    pub time: Finite,
}

/// Payload of `message.part.removed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct PartRemovedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: MessageID,
    #[serde(rename = "partID")]
    pub part_id: PartID,
}

/// Payload of `message.part.delta`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct PartDeltaData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "messageID")]
    pub message_id: MessageID,
    #[serde(rename = "partID")]
    pub part_id: PartID,
    pub field: String,
    pub delta: String,
}

/// Payload of `session.diff`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct DiffData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub diff: Vec<file_diff::Info>,
}

/// Payload of `session.error`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ErrorEventData {
    #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none", default)]
    pub session_id: Option<SessionID>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<AssistantError>,
}

/// `SessionV1.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::{
        Created, Deleted, Diff, ErrorEvent, MessageRemoved, MessageUpdated, PartDelta, PartRemoved,
        PartUpdated, Updated,
    };
    pub use crate::event::Definition;

    /// `SessionV1.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[
        Definition {
            r#type: "session.created",
            durable: Some(crate::event::DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "session.updated",
            durable: Some(crate::event::DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "session.deleted",
            durable: Some(crate::event::DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "message.updated",
            durable: Some(crate::event::DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "message.removed",
            durable: Some(crate::event::DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "message.part.updated",
            durable: Some(crate::event::DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "message.part.removed",
            durable: Some(crate::event::DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "message.part.delta",
            durable: None,
        },
        Definition {
            r#type: "session.diff",
            durable: None,
        },
        Definition {
            r#type: "session.error",
            durable: None,
        },
    ];
}

/// `SessionV1.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
