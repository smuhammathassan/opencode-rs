//! Port of `reference/packages/core/src/tool/glob.ts` and `grep.ts`
//! (V2 core `glob` / `grep` leaves).

use crate::core::tool::{self, CoreContext, CoreTool};
use crate::model::{Content, ToolError};
use crate::ripgrep;
use crate::schema::{opt_prop, prop, Schema};

pub mod glob {
    use super::*;

    pub const NAME: &str = "glob";

    /// `Input` from `reference/packages/core/src/tool/glob.ts:18`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![
                prop(
                    "pattern",
                    Schema::string("Glob pattern to match files against"),
                ),
                opt_prop(
                    "path",
                    Schema::plain_string().with_description(
                        "Relative directory to search. Defaults to the active Location.",
                    ),
                ),
                opt_prop(
                    "limit",
                    Schema::positive_int().with_description("Maximum results to return"),
                ),
            ],
            "glob",
        )
    }

    /// `toModelOutput` from `reference/packages/core/src/tool/glob.ts:32`.
    pub fn to_model_output(output: &[serde_json::Value]) -> String {
        if output.is_empty() {
            return "No files found".to_string();
        }
        output
            .iter()
            .filter_map(|item| item.get("path").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `GlobTool` from `reference/packages/core/src/tool/glob.ts:38`.
    pub fn def() -> CoreTool {
        tool::make(
            "Find files by glob pattern within the active Location. Returns concise relative file resources. Use a relative path to narrow the search and limit to bound the result count.",
            input(),
            entry_array_schema(),
            None,
            None,
            Some(std::sync::Arc::new(|_input, output| {
                let entries = output.as_array().cloned().unwrap_or_default();
                vec![Content::Text {
                    text: to_model_output(&entries),
                }]
            })),
            execute,
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
    ) -> Result<serde_json::Value, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let limit = input
            .get("limit")
            .and_then(|v| v.as_i64())
            .map(|value| value as usize)
            .unwrap_or(usize::MAX);

        context.assert(crate::core::tool::CorePermissionRequest {
            action: NAME.to_string(),
            resources: vec![pattern.clone()],
            save: Some(vec!["*".to_string()]),
            metadata: Some(serde_json::json!({
                "root": path.clone().unwrap_or_else(|| ".".to_string()),
                "path": path,
                "limit": limit,
            })),
            source: source(context),
        })?;

        let cwd =
            crate::util::path_resolve(&context.location_directory, path.as_deref().unwrap_or("."));
        let results = ripgrep::glob(&ripgrep::GlobInput {
            cwd,
            pattern,
            limit,
            hidden: false,
            follow: false,
        })
        .map_err(|_| ToolError::failure("Unable to find files matching pattern"))?;

        let location = context.location_directory.clone();
        let entries: Vec<serde_json::Value> = results
            .into_iter()
            .map(|entry| {
                let absolute = crate::util::path_resolve(&location, &entry.path);
                let relative = crate::util::path_relative(&location, &absolute);
                serde_json::json!({ "path": relative, "type": "file" })
            })
            .collect();
        Ok(serde_json::Value::Array(entries))
    }

    fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
        crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        }
    }
}

pub mod grep {
    use super::*;

    pub const NAME: &str = "grep";

    /// `Input` from `reference/packages/core/src/tool/grep.ts:19`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![
                prop("pattern", Schema::string("Regex pattern to search for in file contents")),
                opt_prop(
                    "path",
                    Schema::plain_string().with_description("Relative directory to search. Defaults to the active Location."),
                ),
                opt_prop(
                    "include",
                    Schema::plain_string().with_description(
                        "File glob to include in the search (for example, \"*.js\" or \"*.{ts,tsx}\")",
                    ),
                ),
                opt_prop("limit", Schema::positive_int().with_description("Maximum matches to return")),
            ],
            "grep",
        )
    }

    /// `toModelOutput` from `reference/packages/core/src/tool/grep.ts:38`.
    pub fn to_model_output(output: &[serde_json::Value]) -> String {
        let mut lines = if output.is_empty() {
            vec!["No files found".to_string()]
        } else {
            vec![format!("Found {} matches", output.len())]
        };
        let mut current = String::new();
        for item in output {
            let path = item
                .pointer("/entry/path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let line = item.get("line").and_then(|v| v.as_i64()).unwrap_or(0);
            let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if current != path {
                if !current.is_empty() {
                    lines.push(String::new());
                }
                current = path.to_string();
                lines.push(format!("{path}:"));
            }
            lines.push(format!("  Line {line}: {text}"));
        }
        lines.join("\n")
    }

    /// `GrepTool` from `reference/packages/core/src/tool/grep.ts:53`.
    pub fn def() -> CoreTool {
        tool::make(
            "Search file contents by regular expression within the active Location or an absolute managed tool-output file. Use a path to narrow the search, include to filter files by glob, and limit to bound the match count. Returns concise file resources, line numbers, and bounded line previews.",
            input(),
            match_array_schema(),
            None,
            None,
            Some(std::sync::Arc::new(|_input, output| {
                let matches = output.as_array().cloned().unwrap_or_default();
                vec![Content::Text {
                    text: to_model_output(&matches),
                }]
            })),
            execute,
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
    ) -> Result<serde_json::Value, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let include = input
            .get("include")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let limit = input
            .get("limit")
            .and_then(|v| v.as_i64())
            .map(|value| value as usize)
            .unwrap_or(usize::MAX);

        context.assert(crate::core::tool::CorePermissionRequest {
            action: NAME.to_string(),
            resources: vec![pattern.clone()],
            save: Some(vec!["*".to_string()]),
            metadata: Some(serde_json::json!({
                "root": ".",
                "path": path,
                "include": include,
                "limit": limit,
            })),
            source: source(context),
        })?;

        let requested =
            crate::util::path_resolve(&context.location_directory, path.as_deref().unwrap_or("."));
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
        let file = if info.as_ref().map(|meta| meta.is_file()) == Some(true) {
            std::path::Path::new(&requested)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        } else {
            None
        };

        let results = ripgrep::grep(&ripgrep::GrepInput {
            cwd,
            pattern,
            file,
            include,
            limit,
        })
        .map_err(|_| ToolError::failure("Unable to grep for pattern"))?;

        let location = context.location_directory.clone();
        let matches: Vec<serde_json::Value> = results
            .into_iter()
            .map(|item| {
                let base = if is_directory {
                    requested.clone()
                } else {
                    std::path::Path::new(&requested)
                        .parent()
                        .map(|parent| parent.to_string_lossy().to_string())
                        .unwrap_or_else(|| requested.clone())
                };
                let absolute = crate::util::path_resolve(&base, &item.entry.path);
                let relative = crate::util::path_relative(&location, &absolute);
                serde_json::json!({
                    "entry": { "path": relative, "type": "file" },
                    "line": item.line,
                    "offset": item.offset,
                    "text": item.text,
                    "submatches": item.submatches.iter().map(|sub| {
                        serde_json::json!({ "text": sub.text, "start": sub.start, "end": sub.end })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(serde_json::Value::Array(matches))
    }

    fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
        crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        }
    }
}

fn entry_array_schema() -> Schema {
    Schema::array(
        Schema::struct_(
            vec![
                prop("path", Schema::plain_string()),
                prop("type", Schema::plain_string()),
            ],
            "entry",
        ),
        "entries",
    )
}

fn match_array_schema() -> Schema {
    Schema::array(
        Schema::struct_(
            vec![
                prop("entry", entry_schema()),
                prop("line", Schema::integer()),
                prop("offset", Schema::integer()),
                prop("text", Schema::plain_string()),
                prop("submatches", Schema::array(submatch_schema(), "submatches")),
            ],
            "match",
        ),
        "matches",
    )
}

fn entry_schema() -> Schema {
    Schema::struct_(
        vec![
            prop("path", Schema::plain_string()),
            prop("type", Schema::plain_string()),
        ],
        "entry",
    )
}

fn submatch_schema() -> Schema {
    Schema::struct_(
        vec![
            prop("text", Schema::plain_string()),
            prop("start", Schema::integer()),
            prop("end", Schema::integer()),
        ],
        "submatch",
    )
}
