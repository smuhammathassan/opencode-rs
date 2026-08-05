//! Port of `reference/packages/opencode/src/tool/write.ts`.

use crate::diff::{create_two_files_patch, trim_diff};
use crate::model::{ExecuteResult, PermissionRequest, ToolError};
use crate::prompts;
use crate::schema::{prop, Schema};
use crate::tool::external_directory;
use crate::util::bom;

/// `Parameters` from `reference/packages/opencode/src/tool/write.ts:20`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "content",
                Schema::string("The content to write to the file"),
            ),
            prop(
                "filePath",
                Schema::string(
                    "The absolute path to the file to write (must be absolute, not relative)",
                ),
            ),
        ],
        "write",
    )
}

/// `WriteTool` from `reference/packages/opencode/src/tool/write.ts:27`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def("write", prompts::WRITE, parameters(), |args, ctx| {
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let filepath = args
            .get("filePath")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if filepath.is_empty() {
            return Err(ToolError::Other("filePath is required".to_string()));
        }

        let instance = ctx.instance.clone().ok_or_else(|| {
            ToolError::Other("InstanceState.context is required for the write tool".to_string())
        })?;
        let filepath = if std::path::Path::new(&filepath).is_absolute() {
            filepath
        } else {
            std::path::Path::new(&instance.directory)
                .join(&filepath)
                .to_string_lossy()
                .to_string()
        };
        external_directory::assert_external_directory_file(ctx, &filepath)?;

        let exists = std::path::Path::new(&filepath).exists();
        let source = if exists {
            bom::read_file(&filepath).unwrap_or((false, String::new()))
        } else {
            (false, String::new())
        };
        let next = bom::split(&content);
        let desired_bom = source.0 || next.0;
        let content_old = source.1;
        let content_new = next.1;

        let diff = trim_diff(&create_two_files_patch(
            &filepath,
            &filepath,
            &content_old,
            &content_new,
        ));
        ctx.ask(PermissionRequest {
            permission: "edit".to_string(),
            patterns: vec![crate::util::path_relative(&instance.worktree, &filepath)],
            always: vec!["*".to_string()],
            metadata: serde_json::json!({
                "filepath": filepath,
                "diff": diff,
            }),
        })?;

        write_with_dirs(&filepath, &bom::join(&content_new, desired_bom))?;

        let mut output = "Wrote file successfully.".to_string();
        // TODO(integration): publish FileSystem.Event.Edited / Watcher.Event.Updated,
        // run formatter (`format.file`), and append LSP diagnostics.
        if let Some(block) = ctx.services.lsp_diagnostics(&filepath)? {
            output.push_str(&format!(
                "\n\nLSP errors detected in this file, please fix:\n{block}"
            ));
        }

        Ok(ExecuteResult {
            title: crate::util::path_relative(&instance.worktree, &filepath),
            metadata: serde_json::json!({
                "diagnostics": {},
                "filepath": filepath,
                "exists": exists,
            }),
            output,
            attachments: None,
        })
    })
}

/// `fs.writeWithDirs` (`reference/packages/core/src/fs-util.ts`).
pub fn write_with_dirs(path: &str, content: &str) -> Result<(), ToolError> {
    let target = std::path::Path::new(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| ToolError::Other(format!("Unable to write {path}: {error}")))?;
    }
    std::fs::write(target, content)
        .map_err(|error| ToolError::Other(format!("Unable to write {path}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::ToolContext;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "content": { "description": "The content to write to the file", "type": "string" },
                    "filePath": { "description": "The absolute path to the file to write (must be absolute, not relative)", "type": "string" }
                },
                "required": ["content", "filePath"],
                "type": "object"
            })
        );
    }

    #[tokio::test]
    async fn writes_new_file() {
        let def = crate::tool::tool::wrap("write", def());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("new.txt");
        let mut ctx = ToolContext {
            instance: Some(crate::model::InstanceContext {
                directory: dir.path().to_string_lossy().to_string(),
                worktree: dir.path().to_string_lossy().to_string(),
            }),
            ..Default::default()
        };

        let result = def
            .execute(
                serde_json::json!({
                    "content": "hello world\n",
                    "filePath": file.to_string_lossy(),
                }),
                &mut ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output, "Wrote file successfully.");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello world\n");
        assert_eq!(result.metadata["exists"], serde_json::json!(false));
        assert_eq!(ctx.asks[0].permission, "edit");
    }
}
