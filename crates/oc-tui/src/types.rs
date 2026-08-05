//! Local mirrors of the `@opencode-ai/sdk/v2` wire types consumed by the TUI.
//!
//! These structs reproduce the JSON shapes generated in
//! `reference/packages/sdk/js/src/v2/gen/types.gen.ts` (which mirrors the
//! opencode server's OpenAPI spec). Once `oc-schema` exposes these types the
//! mirrors here should be replaced.
//!
//! TODO(integration): promote to oc-schema and re-export from there.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFileDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub slug: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "workspaceID")]
    pub workspace_id: Option<String>,
    pub directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "parentID")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<SessionSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<MessageTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<SessionShare>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    pub version: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    pub time: SessionTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<Vec<PermissionRule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert: Option<SessionRevert>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub additions: u64,
    pub deletions: u64,
    pub files: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<SnapshotFileDiff>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionShare {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTime {
    pub created: i64,
    pub updated: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacting: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRevert {
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache: CacheTokens,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheTokens {
    pub read: u64,
    pub write: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: String,
    pub action: String,
}

/// The `Message` union: `UserMessage | AssistantMessage`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum Message {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
}

impl Message {
    pub fn id(&self) -> &str {
        match self {
            Message::User(m) => &m.id,
            Message::Assistant(m) => &m.id,
        }
    }
    pub fn session_id(&self) -> &str {
        match self {
            Message::User(m) => &m.session_id,
            Message::Assistant(m) => &m.session_id,
        }
    }
    pub fn role(&self) -> &'static str {
        match self {
            Message::User(_) => "user",
            Message::Assistant(_) => "assistant",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub time: MessageTimeCreated,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<serde_json::Value>,
    pub agent: String,
    pub model: ModelRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageTimeCreated {
    pub created: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageError {
    pub name: String,
    pub data: serde_json::Value,
}

impl MessageError {
    pub fn message(&self) -> String {
        self.data
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.name.clone())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub time: AssistantMessageTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,
    #[serde(rename = "parentID")]
    pub parent_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub mode: String,
    pub agent: String,
    pub path: MessagePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<bool>,
    pub cost: f64,
    pub tokens: MessageTokens,
    #[serde(default)]
    pub structured: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantMessageTime {
    pub created: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePath {
    pub cwd: String,
    pub root: String,
}

/// The `Part` union. Field ordering and naming follow types.gen.ts.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
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
            Part::Text(p) => &p.id,
            Part::Subtask(p) => &p.id,
            Part::Reasoning(p) => &p.id,
            Part::File(p) => &p.id,
            Part::Tool(p) => &p.id,
            Part::StepStart(p) => &p.id,
            Part::StepFinish(p) => &p.id,
            Part::Snapshot(p) => &p.id,
            Part::Patch(p) => &p.id,
            Part::Agent(p) => &p.id,
            Part::Retry(p) => &p.id,
            Part::Compaction(p) => &p.id,
        }
    }

    pub fn message_id(&self) -> &str {
        match self {
            Part::Text(p) => &p.message_id,
            Part::Subtask(p) => &p.message_id,
            Part::Reasoning(p) => &p.message_id,
            Part::File(p) => &p.message_id,
            Part::Tool(p) => &p.message_id,
            Part::StepStart(p) => &p.message_id,
            Part::StepFinish(p) => &p.message_id,
            Part::Snapshot(p) => &p.message_id,
            Part::Patch(p) => &p.message_id,
            Part::Agent(p) => &p.message_id,
            Part::Retry(p) => &p.message_id,
            Part::Compaction(p) => &p.message_id,
        }
    }

    /// The part `type` tag as used in events.
    pub fn type_name(&self) -> &'static str {
        match self {
            Part::Text(_) => "text",
            Part::Subtask(_) => "subtask",
            Part::Reasoning(_) => "reasoning",
            Part::File(_) => "file",
            Part::Tool(_) => "tool",
            Part::StepStart(_) => "step-start",
            Part::StepFinish(_) => "step-finish",
            Part::Snapshot(_) => "snapshot",
            Part::Patch(_) => "patch",
            Part::Agent(_) => "agent",
            Part::Retry(_) => "retry",
            Part::Compaction(_) => "compaction",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub text: String,
    #[serde(default)]
    pub synthetic: Option<bool>,
    #[serde(default)]
    pub ignored: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<PartTime>,
    #[serde(default)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TextPart {
    pub fn is_synthetic(&self) -> bool {
        self.synthetic.unwrap_or(false)
    }
    pub fn is_ignored(&self) -> bool {
        self.ignored.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartTime {
    pub start: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtaskPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub prompt: String,
    pub description: String,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub text: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    pub time: PartTime,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub mime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<FilePartSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FilePartSource {
    #[serde(rename = "file")]
    File(FileSource),
    #[serde(rename = "symbol")]
    Symbol(SymbolSource),
    #[serde(rename = "resource")]
    Resource(ResourceSource),
}

#[derive(Debug, Clone, Deserialize)]
pub struct FilePartSourceText {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSource {
    pub text: FilePartSourceText,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSource {
    pub text: FilePartSourceText,
    pub path: String,
    pub range: SymbolRange,
    pub name: String,
    pub kind: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SymbolRange {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Position {
    pub line: u64,
    pub character: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSource {
    pub text: FilePartSourceText,
    pub client_name: String,
    pub uri: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub tool: String,
    pub state: ToolState,
    #[serde(default)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ToolPart {
    pub fn state_status(&self) -> &'static str {
        match &self.state {
            ToolState::Pending(_) => "pending",
            ToolState::Running(_) => "running",
            ToolState::Completed(_) => "completed",
            ToolState::Error(_) => "error",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
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

#[derive(Debug, Clone, Deserialize)]
pub struct ToolStatePending {
    pub input: serde_json::Map<String, serde_json::Value>,
    pub raw: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolStateRunning {
    pub input: serde_json::Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub time: Option<ToolStateTime>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolStateTime {
    pub start: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolStateCompleted {
    pub input: serde_json::Map<String, serde_json::Value>,
    pub output: String,
    pub title: String,
    pub metadata: serde_json::Map<String, serde_json::Value>,
    pub time: ToolStateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<FilePart>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolStateError {
    pub input: serde_json::Map<String, serde_json::Value>,
    pub error: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    pub time: ToolStateTime,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    pub cost: f64,
    pub tokens: MessageTokens,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub hash: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PartSourceRange>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartSourceRange {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub attempt: u64,
    pub error: MessageError,
    pub time: MessageTimeCreated,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPart {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overflow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_start_id: Option<String>,
}

/// `{ info, parts }` pair returned by `session.messages`.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionMessageData {
    pub info: Message,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Todo {
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "retry")]
    Retry(RetryStatus),
    #[serde(rename = "busy")]
    Busy,
}

impl SessionStatus {
    pub fn kind(&self) -> &'static str {
        match self {
            SessionStatus::Idle => "idle",
            SessionStatus::Retry(_) => "retry",
            SessionStatus::Busy => "busy",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryStatus {
    pub attempt: u64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<RetryAction>,
    pub next: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryAction {
    pub reason: String,
    pub provider: String,
    pub title: String,
    pub message: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub source: String,
    pub env: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub options: serde_json::Map<String, serde_json::Value>,
    pub models: HashMap<String, Model>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(default)]
    pub api: serde_json::Value,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub capabilities: ModelCapabilities,
    #[serde(default)]
    pub cost: serde_json::Value,
    pub limit: ModelLimit,
    pub status: String,
    #[serde(default)]
    pub options: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub release_date: String,
    #[serde(default)]
    pub variants: Option<HashMap<String, serde_json::Map<String, serde_json::Value>>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelCapabilities {
    #[serde(default)]
    pub temperature: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub attachment: bool,
    #[serde(default)]
    pub toolcall: bool,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub output: serde_json::Value,
    #[serde(default)]
    pub interleaved: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelLimit {
    pub context: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    pub output: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub mode: String,
    #[serde(default)]
    pub native: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub permission: Vec<PermissionRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default)]
    pub options: serde_json::Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Command {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub subtask: Option<bool>,
    #[serde(default)]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
    pub always: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<PermissionTool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionTool {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub questions: Vec<QuestionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<QuestionTool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionInfo {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionTool {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileSystemEntry {
    pub name: String,
    pub path: String,
    pub absolute: String,
    pub ignored: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalCapabilities {
    pub background_subagents: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConsoleState {
    pub console_managed_providers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_org_name: Option<String>,
    pub switchable_org_count: u64,
}

/// Loose mirror of the config GET response; only fields the TUI reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub experimental: Option<ConfigExperimental>,
    #[serde(default)]
    pub tui: Option<serde_json::Value>,
    #[serde(default)]
    pub share: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigExperimental {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_paste_summary: Option<bool>,
}

/// Global event envelope consumed from the `/global/event` SSE stream.
/// From reference/packages/sdk/js/src/v2/gen/types.gen.ts (`GlobalEvent`).
#[derive(Debug, Clone, Deserialize)]
pub struct GlobalEvent {
    pub directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventPayload {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub properties: serde_json::Value,
}

impl EventPayload {
    pub fn props<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_value(self.properties.clone()).ok()
    }
    pub fn props_or<T: serde::de::DeserializeOwned + Default>(&self) -> T {
        serde_json::from_value(self.properties.clone()).unwrap_or_default()
    }
}
