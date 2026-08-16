//! Wire types shared by both tool engines.
//!
//! Mirrors the LLM wire contracts from `reference/packages/llm/src/schema/messages.ts`
//! (ToolDefinition, ToolCall, ToolResultValue, ToolOutput, ToolContent) and the
//! session-facing execute result from `reference/packages/opencode/src/tool/tool.ts`.
//!
//! TODO(integration): promote these types to `oc-schema` / `oc-llm` once those
//! crates grow real contracts.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::future::Future;
use std::pin::Pin;

/// A `Send` boxed future, used by tool `execute` handlers.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// `LLM.ToolDefinition` from `reference/packages/llm/src/schema/messages.ts:224`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: JsonValue,
    #[serde(rename = "outputSchema")]
    pub output_schema: Option<JsonValue>,
}

/// `LLM.Content.ToolCall` from `reference/packages/llm/src/schema/messages.ts:122`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: JsonValue,
}

/// `LLM.ToolContent` from `reference/packages/schema/src/llm.ts:11`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "file")]
    File {
        uri: String,
        mime: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

/// `LLM.ToolResult` from `reference/packages/llm/src/schema/messages.ts:50`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ToolResultValue {
    Json { value: JsonValue },
    Text { value: JsonValue },
    Error { value: JsonValue },
    Content { value: Vec<ToolContent> },
}

/// `LLM.ToolOutput` from `reference/packages/llm/src/schema/messages.ts:85`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub structured: JsonValue,
    pub content: Vec<ToolContent>,
}

impl ToolOutput {
    /// `ToolOutput.make` from `reference/packages/llm/src/schema/messages.ts:91`.
    pub fn make(structured: JsonValue, content: Vec<ToolContent>) -> Self {
        Self {
            structured,
            content,
        }
    }

    /// `ToolOutput.toResultValue` from `reference/packages/llm/src/schema/messages.ts:104`.
    pub fn to_result_value(&self) -> ToolResultValue {
        if self.content.is_empty() {
            return ToolResultValue::Json {
                value: self.structured.clone(),
            };
        }
        if self.content.len() == 1 {
            if let ToolContent::Text { text } = &self.content[0] {
                return ToolResultValue::Text {
                    value: JsonValue::String(text.clone()),
                };
            }
        }
        ToolResultValue::Content {
            value: self.content.clone(),
        }
    }

    /// `ToolOutput.fromResultValue` from `reference/packages/llm/src/schema/messages.ts:92`.
    pub fn from_result_value(result: &ToolResultValue) -> Option<Self> {
        match result {
            ToolResultValue::Json { value } => Some(ToolOutput::make(value.clone(), vec![])),
            ToolResultValue::Text { value } => Some(ToolOutput::make(
                JsonValue::Object(Default::default()),
                vec![ToolContent::Text {
                    text: tool_result_text(value),
                }],
            )),
            ToolResultValue::Content { value } => Some(ToolOutput::make(
                JsonValue::Object(Default::default()),
                value.clone(),
            )),
            ToolResultValue::Error { .. } => None,
        }
    }
}

fn tool_result_text(value: &JsonValue) -> String {
    match value {
        JsonValue::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// `LLM.ToolFailure` recoverable tool error.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolFailure {
    pub message: String,
}

impl ToolFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ToolFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Everything a tool execution can produce. Mirrors `ToolFailure` (recoverable)
/// and the defect channel (`Effect.orDie`) in `reference/packages/opencode/src/tool/tool.ts:145`.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolError {
    /// A recoverable `ToolFailure` fed back to the model as an error result.
    Failure(ToolFailure),
    /// `ToolInvalidArgumentsError` from `reference/packages/opencode/src/tool/tool.ts:24`.
    InvalidArguments {
        tool: String,
        detail: String,
        message: String,
    },
    /// Non-recoverable error (the reference `die`s on these).
    Other(String),
}

impl ToolError {
    pub fn failure(message: impl Into<String>) -> Self {
        ToolError::Failure(ToolFailure::new(message))
    }

    /// `InvalidArgumentsError` message from `reference/packages/opencode/src/tool/tool.ts:32`.
    pub fn invalid_arguments(tool: impl Into<String>, detail: impl Into<String>) -> Self {
        let tool = tool.into();
        let detail = detail.into();
        let message = format!(
            "The {tool} tool was called with invalid arguments: {detail}.\nPlease rewrite the input so it satisfies the expected schema."
        );
        ToolError::InvalidArguments {
            tool,
            detail,
            message,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            ToolError::Failure(f) => &f.message,
            ToolError::InvalidArguments { message, .. } => message,
            ToolError::Other(message) => message,
        }
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ToolError {}

impl From<String> for ToolError {
    fn from(value: String) -> Self {
        ToolError::Other(value)
    }
}

impl From<&str> for ToolError {
    fn from(value: &str) -> Self {
        ToolError::Other(value.to_string())
    }
}

/// A model-facing content part. `Core.Tool.Content` from
/// `reference/packages/core/src/tool/tool.ts:36`.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text {
        text: String,
    },
    File {
        data: String,
        mime: String,
        name: Option<String>,
    },
}

/// `InstanceContext` from `reference/packages/opencode/src/project/instance-context.ts:3`.
#[derive(Debug, Clone)]
pub struct InstanceContext {
    pub directory: String,
    pub worktree: String,
}

impl InstanceContext {
    /// `containsPath` from `reference/packages/opencode/src/project/instance-context.ts:13`.
    pub fn contains_path(&self, filepath: &str) -> bool {
        if crate::util::fs_contains(&self.directory, filepath) {
            return true;
        }
        if self.worktree == "/" {
            return false;
        }
        crate::util::fs_contains(&self.worktree, filepath)
    }
}

/// A loaded skill (`Skill.Info` from `reference/packages/core/src/skill.ts`).
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub location: String,
    pub content: String,
}

/// A request sent from the LSP tool to the host's language-server service.
///
/// Paths and positions are kept in the same shape as the tool contract. The
/// LSP implementation owns conversion to file URIs and zero-based protocol
/// positions, so callers cannot accidentally send editor-facing coordinates
/// directly to a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRequest {
    pub operation: String,
    pub file_path: String,
    pub line: usize,
    pub character: usize,
    pub query: Option<String>,
}

/// Session/service capabilities tools delegate to. The session runner wires
/// real implementations; the default is a permissive stub.
///
/// TODO(integration): replace with `oc-session`/`oc-llm`/`oc-mcp` services.
pub trait ToolServices: Send + Sync {
    /// `SessionTodo.update` (`reference/packages/opencode/src/tool/todo.ts:31`).
    fn todo_update(&self, _session_id: &str, _todos: &JsonValue) -> Result<(), String> {
        Ok(())
    }

    /// `Question.ask` (`reference/packages/opencode/src/tool/question.ts:24`).
    /// Returns the answers array (each answer an array of labels).
    fn question_ask(
        &self,
        _session_id: &str,
        _questions: &JsonValue,
        _tool: Option<(String, String)>,
    ) -> Result<JsonValue, String> {
        Ok(JsonValue::Array(vec![]))
    }

    /// `Skill.require` (`reference/packages/opencode/src/tool/skill.ts:23`).
    fn skill_require(&self, _name: &str) -> Result<SkillInfo, String> {
        Err(format!("Skill not found: {_name}"))
    }

    /// `LSP.hasClients` / `LSP.touchFile` (`reference/packages/opencode/src/tool/lsp.ts`).
    fn lsp_available(&self, _file: &str) -> Result<bool, String> {
        Ok(false)
    }

    /// Executes one LSP tool request through the host's configured language
    /// server manager. The default is an explicit error: returning an empty
    /// result here would make an unconfigured server look like a successful
    /// query and was the source of the old placeholder behavior.
    fn lsp_request(&self, _request: LspRequest) -> BoxFuture<'_, Result<Vec<JsonValue>, String>> {
        Box::pin(async { Err("LSP execution is not configured for this tool host".to_string()) })
    }

    /// `LSP.diagnostics` used by write/edit/apply_patch to append LSP blocks.
    fn lsp_diagnostics(&self, _file: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Session message lookup for the read tool's instruction resolution.
    fn resolve_instructions(
        &self,
        _messages: &[JsonValue],
        _file: &str,
    ) -> Result<Vec<JsonValue>, String> {
        Ok(vec![])
    }

    /// `Config.get` used by the task tool for `subagent_depth`.
    fn subagent_depth(&self) -> Option<usize> {
        None
    }

    /// The number of parent subagent sessions above the active session.
    ///
    /// The task tool keeps the depth guard in the tool layer so callers that
    /// use the v1 tool engine cannot accidentally bypass it.  Production
    /// runners can provide the durable session ancestry here.
    fn subagent_parent_depth(&self, _session_id: &str) -> usize {
        0
    }

    /// Execute or schedule one subagent task.
    ///
    /// This is intentionally a callback-shaped capability rather than a
    /// dependency on the server/session crates.  CLI, embedded, and server
    /// runners can each supply their own child-session implementation while
    /// sharing the permission, depth, and output semantics of the task tool.
    fn execute_subagent(
        &self,
        _request: SubagentRequest,
    ) -> BoxFuture<'static, Result<SubagentResult, String>> {
        Box::pin(async {
            Err("Subagent execution is not configured for this tool runtime".to_string())
        })
    }

    /// Observe a child-session lifecycle transition. Hosts can use this to
    /// publish a completion notification for foreground work or a started
    /// notification for background work without coupling the tool crate to a
    /// particular event transport.
    fn notify_subagent(
        &self,
        _request: &SubagentRequest,
        _result: &SubagentResult,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Cooperatively cancel a child whose parent tool call was aborted.
    fn cancel_subagent(&self, _request: &SubagentRequest) -> Result<(), String> {
        Ok(())
    }

    /// Release host-side transient resources after a terminal child result.
    /// This must not delete the durable child session; the session host owns
    /// that policy.
    fn cleanup_subagent(
        &self,
        _request: &SubagentRequest,
        _session_id: Option<&str>,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Default permissive service implementations.
#[derive(Debug, Clone, Default)]
pub struct NullServices;

impl ToolServices for NullServices {}

/// Request passed from the v1 task tool to the active session runner.
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentRequest {
    pub parent_session_id: String,
    pub parent_message_id: String,
    pub description: String,
    pub prompt: String,
    pub subagent_type: String,
    pub task_id: Option<String>,
    pub command: Option<String>,
    pub background: bool,
}

/// Completed, failed, or background-started subagent output.
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentResult {
    pub session_id: String,
    pub state: String,
    pub summary: Option<String>,
    pub output: String,
    pub metadata: JsonValue,
}

/// `Tool.Context` from `reference/packages/opencode/src/tool/tool.ts:36`.
///
/// `messages` mirrors `SessionV1.WithParts`; the parts are stored as raw JSON
/// until `oc-session` provides the typed contract.
#[derive(Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub message_id: String,
    pub agent: String,
    pub call_id: Option<String>,
    /// Arbitrary `ctx.extra` bag (plugin model info, `promptOps`, ...).
    pub extra: JsonValue,
    pub messages: Vec<JsonValue>,
    /// Set to true when the enclosing session aborts the tool call.
    pub aborted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Published title/metadata for the session UI.
    pub metadata: Vec<Metadata>,
    /// Recorded permission asks (the session layer turns these into prompts).
    pub asks: Vec<PermissionRequest>,
    /// The active agent's `PermissionV1.Ruleset`, used for truncation hints.
    pub agent_permission: Option<Vec<crate::util::Rule>>,
    /// The active project instance (`InstanceState.context`).
    pub instance: Option<InstanceContext>,
    /// Delegate services (todo/question/skill/lsp/session hooks).
    pub services: std::sync::Arc<dyn ToolServices>,
}

/// `ctx.metadata` input from `reference/packages/opencode/src/tool/tool.ts:44`.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub title: Option<String>,
    pub metadata: JsonValue,
}

/// `PermissionV1.Request`-shaped ask, narrowed to what tools emit.
/// From `reference/packages/opencode/src/tool/tool.ts:45`.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub permission: String,
    pub patterns: Vec<String>,
    pub always: Vec<String>,
    pub metadata: JsonValue,
}

impl Default for ToolContext {
    fn default() -> Self {
        ToolContext {
            session_id: String::new(),
            message_id: String::new(),
            agent: String::new(),
            call_id: None,
            extra: JsonValue::Null,
            messages: Vec::new(),
            aborted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            metadata: Vec::new(),
            asks: Vec::new(),
            agent_permission: None,
            instance: None,
            services: std::sync::Arc::new(NullServices),
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("session_id", &self.session_id)
            .field("message_id", &self.message_id)
            .field("agent", &self.agent)
            .field("call_id", &self.call_id)
            .field("metadata_len", &self.metadata.len())
            .field("asks_len", &self.asks.len())
            .field("aborted", &self.is_aborted())
            .finish()
    }
}

impl ToolContext {
    /// `ctx.ask` from `reference/packages/opencode/src/tool/tool.ts:45`.
    /// Records the request for the session layer; the default policy allows.
    pub fn ask(&mut self, request: PermissionRequest) -> Result<(), ToolError> {
        self.asks.push(request);
        Ok(())
    }

    /// `ctx.metadata` from `reference/packages/opencode/src/tool/tool.ts:44`.
    pub fn metadata(&mut self, input: Metadata) -> Result<(), ToolError> {
        self.metadata.push(input);
        Ok(())
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn agent_permission(&self) -> Option<&[crate::util::Rule]> {
        self.agent_permission.as_deref()
    }
}

/// `Tool.ExecuteResult` from `reference/packages/opencode/src/tool/tool.ts:48`.
#[derive(Debug, Clone)]
pub struct ExecuteResult {
    pub title: String,
    pub metadata: JsonValue,
    pub output: String,
    pub attachments: Option<Vec<FilePart>>,
}

/// `SessionV1.FilePart`-shaped attachment from `reference/packages/opencode/src/tool/tool.ts:52`.
#[derive(Debug, Clone)]
pub struct FilePart {
    pub mime: String,
    pub url: String,
    pub filename: Option<String>,
}

/// `Core.Tool.Context` from `reference/packages/core/src/tool/tool.ts:9`.
#[derive(Debug, Clone)]
pub struct CoreContext {
    pub session_id: String,
    pub agent: String,
    pub assistant_message_id: String,
    pub tool_call_id: String,
}
