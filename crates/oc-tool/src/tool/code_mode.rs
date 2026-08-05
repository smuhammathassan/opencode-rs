//! Port of `reference/packages/opencode/src/tool/code-mode.ts`.
//!
//! The confined orchestration interpreter (`@opencode-ai/codemode`) is not
//! ported yet; this module exposes the tool contract (`execute`, schema) and
//! the catalog description helper, and rejects execution until the runtime
//! lands.
//!
//! TODO(integration): port the `@opencode-ai/codemode` interpreter and the MCP
//! child-tool invocation loop (`invokeChildTool`, `projectMcpResult`).

use crate::model::{ExecuteResult, ToolError};
use crate::schema::{prop, Schema};

pub const CODE_MODE_TOOL: &str = "execute";

const DESCRIPTION: &str = "Run a confined orchestration script with access to connected MCP tools.";

/// `Parameters` from `reference/packages/opencode/src/tool/code-mode.ts:16`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![prop(
            "code",
            Schema::string("Script body executed by the confined interpreter."),
        )],
        "execute",
    )
}

/// `CodeModeTool` from `reference/packages/opencode/src/tool/code-mode.ts:188`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def(CODE_MODE_TOOL, DESCRIPTION, parameters(), |_args, ctx| {
        if ctx.is_aborted() {
            return Ok(ExecuteResult {
                title: CODE_MODE_TOOL.to_string(),
                metadata: serde_json::json!({ "toolCalls": [], "error": true }),
                output: "Execution cancelled.".to_string(),
                attachments: None,
            });
        }
        Err(ToolError::Other(
            "Code mode execution is not available in this build yet.".to_string(),
        ))
    })
}
