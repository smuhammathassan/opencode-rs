//! V2 core LSP tool. Hosts provide the process-backed implementation.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::model::{Content, LspRequest, ToolError};
use crate::schema::{opt_prop, prop, Schema};

const OPERATIONS: [&str; 9] = [
    "goToDefinition",
    "findReferences",
    "hover",
    "documentSymbol",
    "workspaceSymbol",
    "goToImplementation",
    "prepareCallHierarchy",
    "incomingCalls",
    "outgoingCalls",
];
pub const NAME: &str = "lsp";

pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "operation",
                Schema::literals(&OPERATIONS, "The LSP operation to perform"),
            ),
            prop(
                "filePath",
                Schema::string("The absolute or relative path to the file"),
            ),
            prop(
                "line",
                Schema::int_ge(1).with_description("The line number (1-based)"),
            ),
            prop(
                "character",
                Schema::int_ge(1).with_description("The character offset (1-based)"),
            ),
            opt_prop("query", Schema::string("Search query for workspaceSymbol")),
        ],
        NAME,
    )
}

pub fn def() -> crate::core::tool::CoreTool {
    crate::core::tool::make(
        "Query the configured language server for definitions, references, hover, symbols, implementations, or call hierarchy items.",
        parameters(),
        Schema::array(Schema::raw(json!({})), "LSP result values"),
        None,
        None,
        Some(std::sync::Arc::new(|_, output| {
            vec![Content::Text { text: serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string()) }]
        })),
        |args, context| {
            let operation = string_arg(&args, "operation")?;
            let input_path = string_arg(&args, "filePath")?;
            let line = args.get("line").and_then(Value::as_i64).unwrap_or(0) as usize;
            let character = args.get("character").and_then(Value::as_i64).unwrap_or(0) as usize;
            let query = args.get("query").and_then(Value::as_str).map(str::to_string);
            let root = absolute(Path::new(&context.location_directory));
            let candidate = if Path::new(&input_path).is_absolute() { PathBuf::from(&input_path) } else { root.join(&input_path) };
            let file = std::fs::canonicalize(&candidate).map_err(|error| ToolError::Other(format!("LSP file is not readable `{}`: {error}", candidate.display())))?;
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            if !crate::util::fs_contains(&root.to_string_lossy(), &file.to_string_lossy()) {
                return Err(ToolError::Other(format!("LSP file must be inside the workspace: {}", file.display())));
            }
            let request = LspRequest { operation, file_path: file.to_string_lossy().into_owned(), line, character, query };
            let service = context.lsp_request.as_ref().ok_or_else(|| ToolError::Other(format!("No LSP server configured for workspace `{}`", root.display())))?;
            let result = service(request).map_err(ToolError::Other)?;
            Ok(Value::Array(result))
        },
    )
}

fn string_arg(args: &Value, name: &str) -> Result<String, ToolError> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ToolError::invalid_arguments(NAME, format!("{name} must be a non-empty string"))
        })
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tool::CoreContext;
    use crate::model::ToolCall;

    #[test]
    fn rejects_paths_outside_workspace() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let mut context = CoreContext {
            session_id: "ses".into(),
            agent: "build".into(),
            assistant_message_id: "msg".into(),
            tool_call_id: "call".into(),
            location_directory: root.path().to_string_lossy().into_owned(),
            asks: Vec::new(),
            subagent_depth: None,
            subagent_parent_depth: std::sync::Arc::new(|_| 0),
            execute_subagent: None,
            lsp_request: None,
        };
        let error = crate::core::tool::settle(&def(), &ToolCall { id: "call".into(), name: NAME.into(), input: json!({"operation":"hover","filePath":outside.path(),"line":1,"character":1}) }, &mut context).unwrap_err();
        assert!(error.to_string().contains("inside the workspace"));
    }
}
