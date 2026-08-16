//! Port of `reference/packages/opencode/src/tool/lsp.ts`.

use crate::model::{ExecuteResult, LspRequest, PermissionRequest, ToolError};
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::external_directory;

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

/// `Parameters` from `reference/packages/opencode/src/tool/lsp.ts:23`.
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
                Schema::int_ge(1)
                    .with_description("The line number (1-based, as shown in editors)"),
            ),
            prop(
                "character",
                Schema::int_ge(1)
                    .with_description("The character offset (1-based, as shown in editors)"),
            ),
            opt_prop(
                "query",
                Schema::string(
                    "Search query for workspaceSymbol. Empty string requests all symbols.",
                ),
            ),
        ],
        "lsp",
    )
}

/// `LspTool` from `reference/packages/opencode/src/tool/lsp.ts:37`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def_async("lsp", prompts::LSP, parameters(), |args, ctx| {
        Box::pin(async move {
            let operation = args
                .get("operation")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let file = args
                .get("filePath")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let line = args.get("line").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
            let character = args.get("character").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let instance = ctx.instance.clone().ok_or_else(|| {
                ToolError::Other("InstanceState.context is required for the lsp tool".to_string())
            })?;
            let file = if std::path::Path::new(&file).is_absolute() {
                file
            } else {
                std::path::Path::new(&instance.directory)
                    .join(&file)
                    .to_string_lossy()
                    .to_string()
            };
            external_directory::assert_external_directory_file(ctx, &file)?;

            let meta = match operation.as_str() {
                "workspaceSymbol" => serde_json::json!({ "operation": operation }),
                "documentSymbol" => serde_json::json!({ "operation": operation, "filePath": file }),
                _ => serde_json::json!({
                    "operation": operation,
                    "filePath": file,
                    "line": line,
                    "character": character,
                }),
            };
            ctx.ask(PermissionRequest {
                permission: "lsp".to_string(),
                patterns: vec!["*".to_string()],
                always: vec!["*".to_string()],
                metadata: meta,
            })?;

            let rel_path = crate::util::path_relative(&instance.worktree, &file);
            let detail = match operation.as_str() {
                "workspaceSymbol" => String::new(),
                "documentSymbol" => rel_path,
                _ => format!("{rel_path}:{line}:{character}"),
            };
            let title = if detail.is_empty() {
                operation.clone()
            } else {
                format!("{operation} {detail}")
            };

            if !std::path::Path::new(&file).exists() {
                return Err(ToolError::Other(format!("File not found: {file}")));
            }

            let available = ctx
                .services
                .lsp_available(&file)
                .map_err(|error| ToolError::Other(error))?;
            if !available {
                return Err(ToolError::Other(
                    "No LSP server available for this file type.".to_string(),
                ));
            }

            let result = ctx
                .services
                .lsp_request(LspRequest {
                    operation: operation.clone(),
                    file_path: file.clone(),
                    line,
                    character,
                    query,
                })
                .await
                .map_err(ToolError::Other)?;

            Ok(ExecuteResult {
                title,
                metadata: serde_json::json!({ "result": result }),
                output: if result.is_empty() {
                    format!("No results found for {operation}")
                } else {
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{result:?}"))
                },
                attachments: None,
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::{BoxFuture, InstanceContext, LspRequest, ToolContext, ToolServices};
    use std::sync::Arc;

    struct FakeLsp;

    impl ToolServices for FakeLsp {
        fn lsp_available(&self, _file: &str) -> Result<bool, String> {
            Ok(true)
        }

        fn lsp_request(
            &self,
            request: LspRequest,
        ) -> BoxFuture<'static, Result<Vec<serde_json::Value>, String>> {
            Box::pin(async move {
                Ok(vec![serde_json::json!({
                    "operation": request.operation,
                    "filePath": request.file_path,
                    "line": request.line,
                    "character": request.character,
                    "query": request.query,
                })])
            })
        }
    }

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "character": {
                        "description": "The character offset (1-based, as shown in editors)",
                        "maximum": 9007199254740991i64,
                        "minimum": 1,
                        "type": "integer"
                    },
                    "filePath": { "description": "The absolute or relative path to the file", "type": "string" },
                    "line": {
                        "description": "The line number (1-based, as shown in editors)",
                        "maximum": 9007199254740991i64,
                        "minimum": 1,
                        "type": "integer"
                    },
                    "operation": {
                        "description": "The LSP operation to perform",
                        "enum": [
                            "goToDefinition",
                            "findReferences",
                            "hover",
                            "documentSymbol",
                            "workspaceSymbol",
                            "goToImplementation",
                            "prepareCallHierarchy",
                            "incomingCalls",
                            "outgoingCalls"
                        ],
                        "type": "string"
                    },
                    "query": {
                        "description": "Search query for workspaceSymbol. Empty string requests all symbols.",
                        "type": "string"
                    }
                },
                "required": ["operation", "filePath", "line", "character"],
                "type": "object"
            })
        );
    }

    #[tokio::test]
    async fn dispatches_to_configured_lsp_service() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut ctx = ToolContext {
            services: Arc::new(FakeLsp),
            instance: Some(InstanceContext {
                directory: temp.path().to_string_lossy().into_owned(),
                worktree: temp.path().to_string_lossy().into_owned(),
            }),
            ..Default::default()
        };
        let result = def()
            .execute(
                serde_json::json!({
                    "operation": "hover",
                    "filePath": "main.rs",
                    "line": 1,
                    "character": 2
                }),
                &mut ctx,
            )
            .await
            .unwrap();
        assert!(result.output.contains("\"operation\": \"hover\""));
        assert!(result.output.contains("\"line\": 1"));
    }
}
