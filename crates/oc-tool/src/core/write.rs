//! Port of `reference/packages/core/src/tool/write.ts` (V2 core `write` leaf).

use crate::core::tool::{self, CoreContext, CoreTool};
use crate::model::{Content, ToolError};
use crate::schema::{prop, Schema};
use crate::util::bom;

pub const NAME: &str = "write";

/// `Input` from `reference/packages/core/src/tool/write.ts:22`.
pub fn input() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "path",
                Schema::string("File path to write. Relative paths resolve within the active Location. Absolute paths inside that Location are accepted; external absolute paths require external_directory approval."),
            ),
            prop("content", Schema::string("Content to write to the file")),
        ],
        "write",
    )
}

/// `toModelOutput` from `reference/packages/core/src/tool/write.ts:38`.
pub fn to_model_output(output: &serde_json::Value) -> String {
    let existed = output
        .get("existed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let resource = output
        .get("resource")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    format!(
        "{} file successfully: {resource}",
        if existed { "Wrote" } else { "Created" }
    )
}

/// `WriteTool` from `reference/packages/core/src/tool/write.ts:47`.
pub fn def() -> CoreTool {
    let tool = tool::make(
        "Write content to one file. Relative paths resolve within the active Location. Absolute paths inside the Location are accepted. Explicit external absolute paths require external_directory approval before edit approval.",
        input(),
        write_output_schema(),
        None,
        None,
        Some(std::sync::Arc::new(|_input, output| {
            vec![Content::Text {
                text: to_model_output(output),
            }]
        })),
        execute,
    );
    tool::with_permission(tool, "edit")
}

pub fn write_output_schema() -> Schema {
    Schema::struct_(
        vec![
            prop("operation", Schema::literals(&["write"], "operation")),
            prop("target", Schema::plain_string()),
            prop("resource", Schema::plain_string()),
            prop("existed", Schema::plain_boolean()),
        ],
        "write",
    )
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
    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

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

    let existed = std::path::Path::new(&target).exists();
    crate::tool::write::write_with_dirs(&target, &content)
        .map_err(|_| ToolError::failure(format!("Unable to write {path}")))?;
    let _ = bom::split(&content);

    Ok(serde_json::json!({
        "operation": "write",
        "target": target,
        "resource": resource,
        "existed": existed,
    }))
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
