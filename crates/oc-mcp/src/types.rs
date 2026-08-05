//! Model Context Protocol schema types (client side).
//!
//! Mirrors the schemas of `@modelcontextprotocol/sdk@1.29.0` `types.js` that the
//! reference consumes in `reference/packages/opencode/src/mcp/index.ts` and
//! `reference/packages/opencode/src/mcp/catalog.ts`. Field names and defaults
//! match the zod output exactly.

use serde::{Deserialize, Serialize};

use crate::jsonrpc::RequestId;

/// Latest MCP protocol version spoken by `@modelcontextprotocol/sdk@1.29.0`.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Protocol versions the SDK accepts in an `initialize` response.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// `{ name, version }` — client/server implementation info.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Implementation {
    pub name: String,
    pub version: String,
}

/// Client capabilities advertised in `initialize`. The reference only enables
/// `roots` (`reference/packages/opencode/src/mcp/index.ts`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<serde_json::Value>,
}

/// Server capabilities from the `initialize` result. Values are kept as raw
/// JSON so `{ tools: {} }` and `{ tools: { listChanged: true } }` are both
/// "truthy" like the reference's `getServerCapabilities()?.tools`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ServerCapabilities {
    pub fn has_tools(&self) -> bool {
        self.tools.is_some()
    }
    pub fn has_prompts(&self) -> bool {
        self.prompts.is_some()
    }
    pub fn has_resources(&self) -> bool {
        self.resources.is_some()
    }
}

/// `initialize` request params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequestParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: Implementation,
}

/// `initialize` result.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// An MCP tool definition as returned by `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `tools/list` result.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Request metadata carrying the progress token. The SDK only emits
/// `_meta.progressToken` when an `onprogress` hook is present.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<RequestId>,
}

/// `tools/call` request params. The reference always sends `arguments` as an
/// object (`(args || {})` in `catalog.ts`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CallToolRequestParams {
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// A content block inside a `CallToolResult`. Tolerant: unknown fields are kept
/// in `extra` so non-standard server payloads round-trip.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentBlock {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceContents>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock {
            r#type: "text".into(),
            text: Some(text.into()),
            mime_type: None,
            data: None,
            resource: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Contents of an embedded resource block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContents {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `tools/call` result. `isError` defaults to `false`, mirroring the SDK's
/// `Schema.optionalWith(Schema.Boolean, { default: () => false })`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An MCP resource from `resources/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `resources/list` result.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    pub resources: Vec<Resource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// A resource template from `resources/templates/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTemplate {
    pub uri_template: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `resources/templates/list` result.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListResourceTemplatesResult {
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: Vec<ResourceTemplate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Argument description of an MCP prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PromptArgument {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// An MCP prompt from `prompts/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `prompts/list` result.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsResult {
    pub prompts: Vec<Prompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `prompts/get` request params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetPromptRequestParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// `prompts/get` result.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetPromptResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `resources/read` request params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadResourceRequestParams {
    pub uri: String,
}

/// `resources/read` result.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Logging levels per MCP spec.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LoggingLevel {
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "notice")]
    Notice,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "critical")]
    Critical,
    #[serde(rename = "alert")]
    Alert,
    #[serde(rename = "emergency")]
    Emergency,
}

/// `notifications/message` params.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoggingMessageNotificationParams {
    pub level: LoggingLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
    pub data: serde_json::Value,
}

/// `notifications/progress` params.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgressNotificationParams {
    pub progress_token: RequestId,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
}

/// A workspace root advertised by the client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `roots/list` request (a server→client request the client must answer).
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListRootsRequestParams {}

/// `roots/list` result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListRootsResult {
    pub roots: Vec<Root>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_roundtrips_with_output_schema() {
        let raw = json!({
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            },
            "outputSchema": { "type": "object" },
            "annotations": { "readOnlyHint": true }
        });
        let tool: Tool = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&tool).unwrap(), raw);
    }

    /// The SDK's `TolerantListToolsResultSchema` omits `outputSchema`; a tool
    /// without it must still parse.
    #[test]
    fn tool_without_output_schema_parses() {
        let raw = json!({
            "name": "simple",
            "inputSchema": { "type": "object" }
        });
        let tool: Tool = serde_json::from_value(raw).unwrap();
        assert_eq!(tool.name, "simple");
        assert_eq!(tool.output_schema, None);
    }

    #[test]
    fn call_tool_params_serializes_exactly() {
        let params = CallToolRequestParams {
            name: "read_file".into(),
            arguments: json!({ "path": "/tmp/a" }),
            meta: Some(RequestMeta {
                progress_token: Some(RequestId::Number(1)),
            }),
        };
        assert_eq!(
            serde_json::to_string(&params).unwrap(),
            r#"{"name":"read_file","arguments":{"path":"/tmp/a"},"_meta":{"progressToken":1}}"#
        );
    }

    #[test]
    fn call_tool_result_defaults_is_error_false() {
        let raw = json!({
            "content": [{ "type": "text", "text": "hello" }]
        });
        let result: CallToolResult = serde_json::from_value(raw).unwrap();
        assert!(!result.is_error);
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({
                "content": [{ "type": "text", "text": "hello" }],
                "isError": false
            })
        );
    }

    #[test]
    fn call_tool_result_preserves_is_error() {
        let raw = json!({
            "content": [],
            "isError": true
        });
        let result: CallToolResult = serde_json::from_value(raw).unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn initialize_result_parses() {
        let raw = json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": { "listChanged": true }, "resources": {} },
            "serverInfo": { "name": "test-server", "version": "1.2.3" },
            "instructions": "Use the tools."
        });
        let result: InitializeResult = serde_json::from_value(raw).unwrap();
        assert_eq!(result.protocol_version, "2025-06-18");
        assert!(result.capabilities.has_tools());
        assert!(result.capabilities.has_resources());
        assert!(!result.capabilities.has_prompts());
        assert_eq!(result.server_info.name, "test-server");
        assert_eq!(result.instructions.as_deref(), Some("Use the tools."));
    }

    #[test]
    fn text_content_block_serializes_exactly() {
        let block = ContentBlock::text("hello");
        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            json!({ "type": "text", "text": "hello" })
        );
    }

    #[test]
    fn list_results_parse_next_cursor() {
        let raw = json!({
            "tools": [{ "name": "a", "inputSchema": { "type": "object" } }],
            "nextCursor": "page2"
        });
        let result: ListToolsResult = serde_json::from_value(raw).unwrap();
        assert_eq!(result.next_cursor.as_deref(), Some("page2"));
    }
}
