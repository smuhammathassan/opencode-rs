//! Port of `reference/packages/opencode/src/tool/glob.ts`.

use crate::model::{ExecuteResult, PermissionRequest, ToolError};
use crate::prompts;
use crate::ripgrep;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::external_directory;

/// `Parameters` from `reference/packages/opencode/src/tool/glob.ts:10`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop("pattern", Schema::string("The glob pattern to match files against")),
            opt_prop(
                "path",
                Schema::string("The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the default behavior. Must be a valid directory path if provided."),
            ),
        ],
        "glob",
    )
}

/// `GlobTool` from `reference/packages/opencode/src/tool/glob.ts:17`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def("glob", prompts::GLOB, parameters(), |args, ctx| {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        ctx.ask(PermissionRequest {
            permission: "glob".to_string(),
            patterns: vec![pattern.clone()],
            always: vec!["*".to_string()],
            metadata: serde_json::json!({ "pattern": pattern, "path": path }),
        })?;

        let instance = ctx.instance.clone().ok_or_else(|| {
            ToolError::Other("InstanceState.context is required for the glob tool".to_string())
        })?;
        let search = match &path {
            Some(path) if std::path::Path::new(path).is_absolute() => path.clone(),
            Some(path) => std::path::Path::new(&instance.directory)
                .join(path)
                .to_string_lossy()
                .to_string(),
            None => instance.directory.clone(),
        };

        let info = std::fs::symlink_metadata(&search).ok();
        if info.as_ref().map(|meta| meta.is_file()) == Some(true) {
            return Err(ToolError::Other(format!(
                "glob path must be a directory: {search}"
            )));
        }

        external_directory::assert_external_directory(
            ctx,
            Some(&search),
            false,
            external_directory::Kind::Directory,
        )?;

        let limit = 100;
        let files = ripgrep::glob(&ripgrep::GlobInput {
            cwd: search.clone(),
            pattern: pattern.clone(),
            limit,
            hidden: false,
            follow: false,
        })
        .map_err(|error| ToolError::Other(format!("Unable to glob for {pattern}: {error}")))?;
        let truncated = files.len() == limit;

        let mut output = Vec::new();
        if files.is_empty() {
            output.push("No files found".to_string());
        } else {
            for file in &files {
                output.push(
                    std::path::Path::new(&search)
                        .join(&file.path)
                        .to_string_lossy()
                        .to_string(),
                );
            }
            if truncated {
                output.push(String::new());
                output.push(format!(
                        "(Results are truncated: showing first {limit} results. Consider using a more specific path or pattern.)"
                    ));
            }
        }

        Ok(ExecuteResult {
            title: crate::util::path_relative(&instance.worktree, &search),
            metadata: serde_json::json!({ "count": files.len(), "truncated": truncated }),
            output: output.join("\n"),
            attachments: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::ToolContext;
    use crate::tool::tool;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "path": {
                        "description": "The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the default behavior. Must be a valid directory path if provided.",
                        "type": "string"
                    },
                    "pattern": { "description": "The glob pattern to match files against", "type": "string" }
                },
                "required": ["pattern"],
                "type": "object"
            })
        );
    }

    #[tokio::test]
    async fn no_files_returns_no_files_found() {
        let def = tool::wrap("glob", def());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("keep.rs"), "fn main() {}").unwrap();
        let mut ctx = ToolContext {
            instance: Some(crate::model::InstanceContext {
                directory: dir.path().to_string_lossy().to_string(),
                worktree: dir.path().to_string_lossy().to_string(),
            }),
            ..Default::default()
        };
        let result = def
            .execute(serde_json::json!({ "pattern": "**/*.zzz" }), &mut ctx)
            .await
            .unwrap();
        assert_eq!(result.output, "No files found");
        assert_eq!(result.metadata["count"], serde_json::json!(0));
    }
}
