//! Port of `reference/packages/core/src/tool/edit.ts` (V2 core `edit` leaf).

use crate::core::tool::{self, CoreContext, CoreTool};
use crate::diff::{create_two_files_patch, diff_lines};
use crate::model::{Content, ToolError};
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::edit::{detect_line_ending, normalize_line_endings};
use crate::util::bom;

pub const NAME: &str = "edit";

/// `Input` from `reference/packages/core/src/tool/edit.ts:24`.
pub fn input() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "path",
                Schema::string("File path to edit. Relative paths resolve within the active Location. Absolute paths inside that Location are accepted; external absolute paths require external_directory approval."),
            ),
            prop("oldString", Schema::string("Exact text to replace")),
            prop("newString", Schema::string("Replacement text, which must differ from oldString")),
            opt_prop(
                "replaceAll",
                Schema::boolean("Replace all exact occurrences of oldString (default false)"),
            ),
        ],
        "edit",
    )
}

/// `Output` from `reference/packages/core/src/tool/edit.ts:36`.
pub fn output_schema() -> Schema {
    Schema::struct_(
        vec![
            prop("files", Schema::array(file_diff_schema(), "files")),
            prop("replacements", Schema::integer()),
        ],
        "edit",
    )
}

pub fn file_diff_schema() -> Schema {
    Schema::struct_(
        vec![
            opt_prop("file", Schema::plain_string()),
            opt_prop("patch", Schema::plain_string()),
            prop("additions", Schema::number()),
            prop("deletions", Schema::number()),
            opt_prop(
                "status",
                Schema::literals(&["added", "deleted", "modified"], "status"),
            ),
        ],
        "file-diff",
    )
}

/// `toModelOutput` from `reference/packages/core/src/tool/edit.ts:73`.
pub fn to_model_output(output: &serde_json::Value, old_string: &str, new_string: &str) -> String {
    let file = output
        .pointer("/files/0/file")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let replacements = output
        .get("replacements")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mut lines = vec![
        format!("Edited file successfully: {file}"),
        format!("Replacements: {replacements}"),
        "```diff".to_string(),
    ];
    lines.extend(preview_lines(old_string, '-'));
    lines.extend(preview_lines(new_string, '+'));
    lines.push("```".to_string());
    lines.join("\n")
}

fn preview_lines(value: &str, prefix: char) -> Vec<String> {
    let lines: Vec<String> = normalize_line_endings(value)
        .split('\n')
        .map(|line| {
            if line.len() > 240 {
                format!("{prefix}{}...", &line[..240])
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect();
    let shown = lines.iter().take(6).cloned().collect::<Vec<_>>();
    if lines.len() > shown.len() {
        let mut with_ellipsis = shown;
        with_ellipsis.push(format!("{prefix}..."));
        with_ellipsis
    } else {
        shown
    }
}

/// `EditTool` from `reference/packages/core/src/tool/edit.ts:90`.
pub fn def() -> CoreTool {
    let tool = tool::make(
        "Replace exact text in one file. Relative paths resolve within the active Location. Absolute paths inside the Location are accepted. Explicit external absolute paths require external_directory approval before edit approval.",
        input(),
        output_schema(),
        None,
        None,
        Some(std::sync::Arc::new(|input, output| {
            let old_string = input.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
            let new_string = input.get("newString").and_then(|v| v.as_str()).unwrap_or("");
            vec![Content::Text {
                text: to_model_output(output, old_string, new_string),
            }]
        })),
        execute,
    );
    tool::with_permission(tool, "edit")
}

fn execute(
    input: serde_json::Value,
    context: &mut CoreContext,
) -> Result<serde_json::Value, ToolError> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let old_string = input
        .get("oldString")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let new_string = input
        .get("newString")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let replace_all = input
        .get("replaceAll")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if old_string == new_string {
        return Err(ToolError::failure(
            "No changes to apply: oldString and newString are identical.",
        ));
    }
    if old_string.is_empty() {
        return Err(ToolError::failure(
            "oldString must not be empty. Use write to create or overwrite a file.",
        ));
    }

    let target = crate::util::path_resolve(&context.location_directory, &path);
    let resource = crate::util::path_relative(&context.location_directory, &target);
    if !crate::util::fs_contains(&context.location_directory, &target) {
        context.assert(crate::core::tool::CorePermissionRequest {
            action: "external_directory".to_string(),
            resources: vec![format!("{}/*", parent_dir(&target))],
            save: None,
            metadata: Some(serde_json::json!({ "filepath": target })),
            source: source(context),
        })?;
    }
    context.assert(crate::core::tool::CorePermissionRequest {
        action: "edit".to_string(),
        resources: vec![resource.clone()],
        save: Some(vec!["*".to_string()]),
        metadata: None,
        source: source(context),
    })?;

    let bytes =
        std::fs::read(&target).map_err(|_| ToolError::failure(format!("Unable to edit {path}")))?;
    let (bom_present, text) = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        (true, String::from_utf8_lossy(&bytes[3..]).to_string())
    } else {
        (false, String::from_utf8_lossy(&bytes).to_string())
    };
    let ending = detect_line_ending(&text);
    let old = if ending == "\n" {
        old_string
    } else {
        old_string.replace('\n', "\r\n")
    };
    let replacement = if ending == "\n" {
        new_string
    } else {
        new_string.replace('\n', "\r\n")
    };

    let replacements = count_occurrences(&text, &old);
    if replacements == 0 {
        return Err(ToolError::failure(
            "Could not find oldString in the file. It must match exactly, including whitespace and indentation.",
        ));
    }
    if replacements > 1 && !replace_all {
        return Err(ToolError::failure(
            "Found multiple exact matches for oldString. Provide more surrounding context or set replaceAll to true.",
        ));
    }

    let replaced = if replace_all {
        text.replace(&old, &replacement)
    } else {
        text.replacen(&old, &replacement, 1)
    };
    let mut additions = 0;
    let mut deletions = 0;
    for part in diff_lines(&text, &replaced) {
        if part.added {
            additions += part.count;
        }
        if part.removed {
            deletions += part.count;
        }
    }
    let next = bom::split(&replaced);
    let joined = if bom_present || next.0 {
        format!("\u{feff}{}", next.1)
    } else {
        next.1.clone()
    };
    crate::tool::write::write_with_dirs(&target, &joined)
        .map_err(|_| ToolError::failure(format!("Unable to edit {path}")))?;

    let patch = create_two_files_patch(&resource, &resource, &text, &replaced);
    Ok(serde_json::json!({
        "files": [{
            "file": resource,
            "patch": patch,
            "status": "modified",
            "additions": additions,
            "deletions": deletions,
        }],
        "replacements": replacements,
    }))
}

fn count_occurrences(content: &str, search: &str) -> usize {
    if search.is_empty() {
        return content.len() + 1;
    }
    content.matches(search).count()
}

fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
    crate::core::tool::CorePermissionSource {
        message_id: context.assistant_message_id.clone(),
        call_id: context.tool_call_id.clone(),
    }
}

fn parent_dir(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
