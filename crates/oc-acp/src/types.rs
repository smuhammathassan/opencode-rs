//! ACP (Agent Client Protocol) wire types.
//!
//! Structs mirror the JSON shapes defined by `@agentclientprotocol/sdk` 0.21.0
//! (`schema/schema.json`). Field ordering matches the object literals emitted by
//! the reference implementation in `reference/packages/opencode/src/acp/` so
//! that serialized JSON is byte-identical.

use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::{Map, Value};

/// The ACP protocol version negotiated during initialization.
pub const PROTOCOL_VERSION: u32 = 1;

/// `sessionId` values are plain strings.
pub type SessionId = String;

/// `configId` values are plain strings.
pub type SessionConfigId = String;

/// `SessionConfigValueId` values are plain strings.
pub type SessionConfigValueId = String;

/// `toolCallId` values are plain strings.
pub type ToolCallId = String;

/// The sender or recipient of messages in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "user")]
    User,
}

/// Optional annotations attached to content blocks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<Role>>,
}

/// A content block: text, image, resource link or embedded resource.
///
/// From reference/packages/opencode/src/acp/content.ts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(TextContent),
    Image(ImageContent),
    #[serde(rename = "resource_link")]
    ResourceLink(ResourceLink),
    #[serde(rename = "resource")]
    Resource(EmbeddedResource),
}

/// Text provided to or from an LLM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextContent {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

/// An image provided to or from an LLM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

/// A resource reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLink {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

/// Complete resource contents embedded in the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedResource {
    pub resource: EmbeddedResourceResource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

/// Resource content that can be embedded in a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddedResourceResource {
    Text(TextResourceContents),
    Blob(BlobResourceContents),
}

/// Text-based resource contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextResourceContents {
    pub text: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Binary resource contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobResourceContents {
    pub blob: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A streamed item of content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub content: ContentBlock,
}

/// Reasons why an agent stops processing a prompt turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
}

/// Categories of tools that can be invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolKind {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "edit")]
    Edit,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "move")]
    Move,
    #[serde(rename = "search")]
    Search,
    #[serde(rename = "execute")]
    Execute,
    #[serde(rename = "think")]
    Think,
    #[serde(rename = "fetch")]
    Fetch,
    #[serde(rename = "switch_mode")]
    SwitchMode,
    #[serde(rename = "other")]
    Other,
}

/// Execution status of a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// A file location being accessed or modified by a tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallLocation {
    pub path: String,
}

/// Content produced by a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolCallContent {
    #[serde(rename = "content")]
    Content { content: ContentBlock },
    #[serde(rename = "diff")]
    Diff(Diff),
}

/// A diff representing file modifications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    pub new_text: String,
}

/// Represents a tool call that the language model has requested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub tool_call_id: ToolCallId,
    pub title: String,
    pub kind: ToolKind,
    pub status: ToolCallStatus,
    pub locations: Vec<ToolCallLocation>,
    pub raw_input: Value,
}

/// An update to an existing tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallUpdate {
    pub tool_call_id: ToolCallId,
    pub status: ToolCallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ToolKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<ToolCallLocation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ToolCallContent>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<Value>,
}

/// Available commands are ready or have changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableCommandsUpdate {
    pub available_commands: Vec<AvailableCommand>,
}

/// Information about a command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableCommand {
    pub name: String,
    pub description: String,
}

/// Cost information for a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub amount: f64,
    pub currency: String,
}

/// Context window and cost update for a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageUpdate {
    pub used: u64,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

/// Different types of updates sent during session processing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "sessionUpdate")]
pub enum SessionUpdate {
    #[serde(rename = "user_message_chunk")]
    UserMessageChunk(ContentChunk),
    #[serde(rename = "agent_message_chunk")]
    AgentMessageChunk(ContentChunk),
    #[serde(rename = "agent_thought_chunk")]
    AgentThoughtChunk(ContentChunk),
    #[serde(rename = "tool_call")]
    ToolCall(ToolCall),
    #[serde(rename = "tool_call_update")]
    ToolCallUpdate(ToolCallUpdate),
    #[serde(rename = "available_commands_update")]
    AvailableCommandsUpdate(AvailableCommandsUpdate),
    #[serde(rename = "usage_update")]
    UsageUpdate(UsageUpdate),
}

/// Token usage information for a prompt turn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_write_tokens: Option<u64>,
}

/// An HTTP header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// An environment variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVariable {
    pub name: String,
    pub value: String,
}

/// HTTP transport configuration for MCP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerHttp {
    pub name: String,
    pub url: String,
    pub headers: Vec<HttpHeader>,
}

/// SSE transport configuration for MCP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerSse {
    pub name: String,
    pub url: String,
    pub headers: Vec<HttpHeader>,
}

/// Stdio transport configuration for MCP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStdio {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVariable>,
}

/// Configuration for connecting to an MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServer {
    Http(McpServerHttp),
    Sse(McpServerSse),
    Stdio(McpServerStdio),
}

impl McpServer {
    /// The human-readable name identifying the server.
    pub fn name(&self) -> &str {
        match self {
            McpServer::Http(server) => &server.name,
            McpServer::Sse(server) => &server.name,
            McpServer::Stdio(server) => &server.name,
        }
    }
}

/// The type of permission option being presented to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

/// An option presented to the user when requesting permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: PermissionOptionKind,
}

/// The user's decision on a permission request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome")]
pub enum RequestPermissionOutcome {
    #[serde(rename = "cancelled")]
    Cancelled(CancelledOutcome),
    #[serde(rename = "selected")]
    Selected(SelectedPermissionOutcome),
}

/// The prompt turn was cancelled before the user responded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelledOutcome {}

/// The user selected one of the provided options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPermissionOutcome {
    pub option_id: String,
}

/// Response to a permission request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionResponse {
    pub outcome: RequestPermissionOutcome,
}

/// Request for `session/request_permission` (client notification).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionRequest {
    pub session_id: SessionId,
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
}

/// Request for `fs/write_text_file` (client notification).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteTextFileRequest {
    pub session_id: SessionId,
    pub path: String,
    pub content: String,
}

/// A session configuration option selector and its current state.
///
/// Only the `select` variant is produced by opencode. Fields are ordered to
/// match the object literals in `reference/packages/opencode/src/acp/config-option.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigOption {
    pub id: SessionConfigId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub r#type: String,
    pub current_value: SessionConfigValueId,
    pub options: Vec<SessionConfigSelectOption>,
}

/// A possible value for a session configuration option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigSelectOption {
    pub value: SessionConfigValueId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Metadata about the implementation of a client or agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Implementation {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub version: String,
}

/// Client capabilities (only `_meta` is consumed by opencode).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Request for the `initialize` method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub protocol_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_capabilities: Option<ClientCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<Implementation>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Authentication methods supported by the agent.
///
/// opencode always emits the `agent` variant (no `type` field).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethod {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// MCP capabilities supported by the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sse: Option<bool>,
}

/// Prompt capabilities supported by the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded_context: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<bool>,
}

/// Whether the agent supports `session/close`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCloseCapabilities {}

/// Whether the agent supports `session/fork`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionForkCapabilities {}

/// Whether the agent supports `session/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionListCapabilities {}

/// Whether the agent supports `session/resume`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionResumeCapabilities {}

/// Session capabilities supported by the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close: Option<SessionCloseCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork: Option<SessionForkCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<SessionListCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<SessionResumeCapabilities>,
}

/// Capabilities supported by the agent, advertised during initialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_session: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_capabilities: Option<McpCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_capabilities: Option<PromptCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_capabilities: Option<SessionCapabilities>,
}

/// Response to the `initialize` method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub protocol_version: u32,
    pub agent_capabilities: AgentCapabilities,
    pub auth_methods: Vec<AuthMethod>,
    pub agent_info: Implementation,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Request for the `authenticate` method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateRequest {
    pub method_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response to the `authenticate` method.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateResponse {
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Request for `session/new`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionRequest {
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response from creating a new session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionResponse {
    pub session_id: SessionId,
    pub config_options: Vec<SessionConfigOption>,
}

/// Request for `session/load`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionRequest {
    pub mcp_servers: Vec<McpServer>,
    pub cwd: String,
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response from loading an existing session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionResponse {
    pub config_options: Vec<SessionConfigOption>,
}

/// Request for `session/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Information about a session returned by `session/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Response from listing sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Request for `session/resume`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSessionRequest {
    pub session_id: SessionId,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response from resuming an existing session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSessionResponse {
    pub config_options: Vec<SessionConfigOption>,
}

/// Request for `session/close`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseSessionRequest {
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response from closing a session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseSessionResponse {
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Request for `session/fork`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkSessionRequest {
    pub session_id: SessionId,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response from forking an existing session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkSessionResponse {
    pub session_id: SessionId,
    pub config_options: Vec<SessionConfigOption>,
}

/// The value of a session configuration option.
///
/// Per `zSetSessionConfigOptionRequest` in the ACP SDK, the value_id form is a
/// plain string and the boolean form carries `type: "boolean"` at the request
/// top level with a boolean `value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigOptionValue {
    /// A boolean `value` (`type: "boolean"` appears at the request top level).
    Boolean(bool),
    /// A plain string value id.
    ValueId(String),
}

/// Request for `session/set_config_option`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionConfigOptionRequest {
    pub session_id: SessionId,
    pub config_id: SessionConfigId,
    pub value: ConfigOptionValue,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response to `session/set_config_option`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionConfigOptionResponse {
    pub config_options: Vec<SessionConfigOption>,
}

/// Request for `session/set_mode`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModeRequest {
    pub session_id: SessionId,
    pub mode_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response to `session/set_mode`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModeResponse {
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Request for `session/set_model`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModelRequest {
    pub session_id: SessionId,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response to `session/set_model`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModelResponse {
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Request for `session/prompt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    pub session_id: SessionId,
    pub prompt: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// Response from processing a user prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    pub stop_reason: StopReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message_id: Option<String>,
    #[serde(rename = "_meta")]
    pub _meta: Map<String, Value>,
}

/// Notification to cancel ongoing operations for a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelNotification {
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Map<String, Value>>,
}

/// The JSON-RPC error object produced by the ACP SDK's `RequestError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RequestError {
    /// Error code for parse errors.
    pub const PARSE_ERROR: i32 = -32700;
    /// Error code for invalid requests.
    pub const INVALID_REQUEST: i32 = -32600;
    /// Error code for unknown methods.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Error code for invalid params.
    pub const INVALID_PARAMS: i32 = -32602;
    /// Error code for internal errors.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// Error code for missing authentication.
    pub const AUTH_REQUIRED: i32 = -32000;

    /// `RequestError.invalidParams(...)` from the ACP SDK.
    pub fn invalid_params(data: Option<Value>, additional_message: Option<&str>) -> Self {
        Self::new(
            Self::INVALID_PARAMS,
            with_suffix("Invalid params", additional_message),
            data,
        )
    }

    /// `RequestError.parseError(...)` from the ACP JSON-RPC transport.
    pub fn parse_error(data: Option<Value>, additional_message: Option<&str>) -> Self {
        Self::new(
            Self::PARSE_ERROR,
            with_suffix("Parse error", additional_message),
            data,
        )
    }

    /// `RequestError.authRequired(...)` from the ACP SDK.
    pub fn auth_required(data: Option<Value>, additional_message: Option<&str>) -> Self {
        Self::new(
            Self::AUTH_REQUIRED,
            with_suffix("Authentication required", additional_message),
            data,
        )
    }

    /// `RequestError.methodNotFound(method)` from the ACP SDK.
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            Self::METHOD_NOT_FOUND,
            format!("\"Method not found\": {method}"),
            Some(json!({ "method": method })),
        )
    }

    /// `RequestError.internalError(...)` from the ACP SDK.
    pub fn internal_error(data: Option<Value>, additional_message: Option<&str>) -> Self {
        Self::new(
            Self::INTERNAL_ERROR,
            with_suffix("Internal error", additional_message),
            data,
        )
    }

    fn new(code: i32, message: String, data: Option<Value>) -> Self {
        Self {
            code,
            message,
            data,
        }
    }
}

fn with_suffix(base: &str, suffix: Option<&str>) -> String {
    match suffix {
        Some(suffix) => format!("{base}: {suffix}"),
        None => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_content(text: &str) -> ContentBlock {
        ContentBlock::Text(TextContent {
            text: text.into(),
            annotations: None,
        })
    }

    #[test]
    fn session_update_agent_message_chunk_golden() {
        let update = SessionUpdate::AgentMessageChunk(ContentChunk {
            message_id: Some("m1".into()),
            content: text_content("hello"),
        });
        assert_eq!(
            serde_json::to_value(&update).unwrap(),
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "m1",
                "content": { "type": "text", "text": "hello" }
            })
        );
    }

    #[test]
    fn session_update_tool_call_golden() {
        let update = SessionUpdate::ToolCall(ToolCall {
            tool_call_id: "c1".into(),
            title: "ls".into(),
            kind: ToolKind::Execute,
            status: ToolCallStatus::Pending,
            locations: vec![ToolCallLocation {
                path: "/cwd".into(),
            }],
            raw_input: json!({ "command": "ls" }),
        });
        assert_eq!(
            serde_json::to_string(&update).unwrap(),
            r#"{"sessionUpdate":"tool_call","toolCallId":"c1","title":"ls","kind":"execute","status":"pending","locations":[{"path":"/cwd"}],"rawInput":{"command":"ls"}}"#
        );
    }

    #[test]
    fn session_update_tool_call_update_golden() {
        let update = SessionUpdate::ToolCallUpdate(ToolCallUpdate {
            tool_call_id: "c1".into(),
            status: ToolCallStatus::InProgress,
            kind: Some(ToolKind::Execute),
            title: Some("ls".into()),
            locations: Some(vec![ToolCallLocation {
                path: "/cwd".into(),
            }]),
            raw_input: Some(json!({ "command": "ls", "cwd": "/cwd" })),
            content: Some(vec![ToolCallContent::Content {
                content: text_content("out"),
            }]),
            raw_output: None,
        });
        assert_eq!(
            serde_json::to_string(&update).unwrap(),
            r#"{"sessionUpdate":"tool_call_update","toolCallId":"c1","status":"in_progress","kind":"execute","title":"ls","locations":[{"path":"/cwd"}],"rawInput":{"command":"ls","cwd":"/cwd"},"content":[{"type":"content","content":{"type":"text","text":"out"}}]}"#
        );
    }

    #[test]
    fn session_update_available_commands_golden() {
        let update = SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate {
            available_commands: vec![AvailableCommand {
                name: "compact".into(),
                description: "Summarize the session".into(),
            }],
        });
        assert_eq!(
            serde_json::to_string(&update).unwrap(),
            r#"{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"compact","description":"Summarize the session"}]}"#
        );
    }

    #[test]
    fn session_update_usage_golden() {
        let update = SessionUpdate::UsageUpdate(UsageUpdate {
            used: 100,
            size: 200_000,
            cost: Some(Cost {
                amount: 0.42,
                currency: "USD".into(),
            }),
        });
        assert_eq!(
            serde_json::to_string(&update).unwrap(),
            r#"{"sessionUpdate":"usage_update","used":100,"size":200000,"cost":{"amount":0.42,"currency":"USD"}}"#
        );
    }

    #[test]
    fn prompt_response_golden() {
        let response = PromptResponse {
            stop_reason: StopReason::EndTurn,
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                thought_tokens: None,
                cached_read_tokens: None,
                cached_write_tokens: None,
            }),
            user_message_id: Some("msg-1".into()),
            _meta: Map::new(),
        };
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"stopReason":"end_turn","usage":{"inputTokens":10,"outputTokens":20,"totalTokens":30},"userMessageId":"msg-1","_meta":{}}"#
        );
    }

    #[test]
    fn initialize_request_decode() {
        let request: InitializeRequest = serde_json::from_str(
            r#"{"protocolVersion":1,"clientCapabilities":{"_meta":{"terminal-auth":true}},"clientInfo":{"name":"zed","version":"0.1"}}"#,
        )
        .unwrap();
        assert_eq!(request.protocol_version, 1);
        let terminal = request
            .client_capabilities
            .unwrap()
            ._meta
            .unwrap()
            .get("terminal-auth")
            .and_then(Value::as_bool);
        assert_eq!(terminal, Some(true));
    }

    #[test]
    fn set_session_config_option_value_decode() {
        let request: SetSessionConfigOptionRequest = serde_json::from_str(
            r#"{"sessionId":"s1","configId":"model","value":"anthropic/claude-sonnet-4"}"#,
        )
        .unwrap();
        assert!(matches!(
            request.value,
            ConfigOptionValue::ValueId(value) if value == "anthropic/claude-sonnet-4"
        ));
        let boolean: SetSessionConfigOptionRequest = serde_json::from_str(
            r#"{"sessionId":"s1","configId":"x","type":"boolean","value":true}"#,
        )
        .unwrap();
        assert!(matches!(boolean.value, ConfigOptionValue::Boolean(true)));
    }

    #[test]
    fn mcp_server_decode() {
        let server: McpServer = serde_json::from_str(
            r#"{"name":"mcp","command":"npx","args":["-y","pkg"],"env":[{"name":"A","value":"b"}]}"#,
        )
        .unwrap();
        assert_eq!(server.name(), "mcp");
        let remote: McpServer = serde_json::from_str(
            r#"{"type":"http","name":"remote","url":"https://x","headers":[{"name":"K","value":"v"}]}"#,
        )
        .unwrap();
        assert_eq!(remote.name(), "remote");
    }

    #[test]
    fn request_permission_outcome_decode() {
        let response: RequestPermissionResponse =
            serde_json::from_str(r#"{"outcome":{"outcome":"selected","optionId":"always"}}"#)
                .unwrap();
        match &response.outcome {
            RequestPermissionOutcome::Selected(selected) => {
                assert_eq!(selected.option_id, "always")
            }
            _ => panic!("expected selected"),
        }
    }
}
