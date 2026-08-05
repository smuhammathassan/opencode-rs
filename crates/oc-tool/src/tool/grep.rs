//! Port of `reference/packages/opencode/src/tool/grep.ts`.

use crate::model::{ExecuteResult, PermissionRequest, ToolError};
use crate::prompts;
use crate::ripgrep;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::external_directory;
use crate::util::path_resolve;

/// `Parameters` from `reference/packages/opencode/src/tool/grep.ts:10`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "pattern",
                Schema::string("The regex pattern to search for in file contents"),
            ),
            opt_prop(
                "path",
                Schema::string(
                    "The directory to search in. Defaults to the current working directory.",
                ),
            ),
            opt_prop(
                "include",
                Schema::string(
                    "File pattern to include in the search (e.g. \"*.js\", \"*.{ts,tsx}\")",
                ),
            ),
        ],
        "grep",
    )
}

const EMPTY_TITLE: &str = "";

/// `GrepTool` from `reference/packages/opencode/src/tool/grep.ts:20`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def("grep", prompts::GREP, parameters(), |args, ctx| {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if pattern.is_empty() {
            return Err(ToolError::Other("pattern is required".to_string()));
        }
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let include = args
            .get("include")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        ctx.ask(PermissionRequest {
            permission: "grep".to_string(),
            patterns: vec![pattern.clone()],
            always: vec!["*".to_string()],
            metadata: serde_json::json!({ "pattern": pattern, "path": path, "include": include }),
        })?;

        let instance = ctx.instance.clone().ok_or_else(|| {
            ToolError::Other("InstanceState.context is required for the grep tool".to_string())
        })?;
        let directory = &instance.directory;
        let requested = match &path {
            Some(path) if std::path::Path::new(path).is_absolute() => path.clone(),
            Some(path) => std::path::Path::new(directory)
                .join(path)
                .to_string_lossy()
                .to_string(),
            None => directory.clone(),
        };
        let requested_info = std::fs::symlink_metadata(&requested).ok();
        let kind = if requested_info.as_ref().map(|meta| meta.is_dir()) == Some(true) {
            external_directory::Kind::Directory
        } else {
            external_directory::Kind::File
        };
        external_directory::assert_external_directory(ctx, Some(&requested), false, kind)?;

        let info = std::fs::symlink_metadata(&requested).ok();
        let is_directory = info.as_ref().map(|meta| meta.is_dir()) == Some(true);
        let cwd = if is_directory {
            requested.clone()
        } else {
            std::path::Path::new(&requested)
                .parent()
                .map(|parent| parent.to_string_lossy().to_string())
                .unwrap_or_else(|| requested.clone())
        };

        let result = ripgrep::grep(&ripgrep::GrepInput {
            cwd,
            pattern: pattern.clone(),
            include,
            file: None,
            limit: 100,
        })
        .map_err(|error| ToolError::Other(format!("Unable to grep for {pattern}: {error}")))?;
        if result.is_empty() {
            return Ok(empty(pattern));
        }

        let base = if is_directory {
            requested.clone()
        } else {
            std::path::Path::new(&requested)
                .parent()
                .map(|parent| parent.to_string_lossy().to_string())
                .unwrap_or_else(|| requested.clone())
        };
        let rows: Vec<(String, i64, String)> = result
            .iter()
            .map(|item| {
                (
                    path_resolve(&base, &item.entry.path),
                    item.line,
                    item.text.clone(),
                )
            })
            .collect();

        let limit = 100;
        let truncated = rows.len() == limit;
        let total = rows.len();
        let has_more = truncated || result.len() == limit;

        let mut output = vec![format!(
            "Found {total} matches{}",
            if has_more {
                " (more matches available)"
            } else {
                ""
            }
        )];
        let mut current = String::new();
        for (row_path, line, text) in &rows {
            if current != *row_path {
                if !current.is_empty() {
                    output.push(String::new());
                }
                current = row_path.clone();
                output.push(format!("{row_path}:"));
            }
            output.push(format!("  Line {line}: {text}"));
        }
        if truncated {
            output.push(String::new());
            output.push(
                "(Results truncated. Consider using a more specific path or pattern.)".to_string(),
            );
        }

        Ok(ExecuteResult {
            title: pattern,
            metadata: serde_json::json!({ "matches": total, "truncated": truncated }),
            output: output.join("\n"),
            attachments: None,
        })
    })
}

fn empty(pattern: String) -> ExecuteResult {
    ExecuteResult {
        title: if pattern.is_empty() {
            EMPTY_TITLE.to_string()
        } else {
            pattern
        },
        metadata: serde_json::json!({ "matches": 0, "truncated": false }),
        output: "No files found".to_string(),
        attachments: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "include": {
                        "description": "File pattern to include in the search (e.g. \"*.js\", \"*.{ts,tsx}\")",
                        "type": "string"
                    },
                    "path": {
                        "description": "The directory to search in. Defaults to the current working directory.",
                        "type": "string"
                    },
                    "pattern": { "description": "The regex pattern to search for in file contents", "type": "string" }
                },
                "required": ["pattern"],
                "type": "object"
            })
        );
    }
}
