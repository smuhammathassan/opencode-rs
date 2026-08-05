//! Port of `reference/packages/core/src/tool/read.ts` (V2 core `read` leaf).

use crate::core::read_filesystem::{self, PageInput};
use crate::core::tool::{self, CoreContext, CoreTool};
use crate::model::{Content, ToolError};
use crate::schema::{opt_prop, prop, Schema};

pub const NAME: &str = "read";

const SUPPORTED_IMAGE_MIMES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

/// `Input` from `reference/packages/core/src/tool/read.ts:18`.
pub fn input() -> Schema {
    Schema::struct_(
        vec![
            prop("path", Schema::plain_string()),
            opt_prop(
                "offset",
                Schema::positive_int().with_description(
                    "The 1-based directory entry or text line offset to start reading from",
                ),
            ),
            opt_prop(
                "limit",
                Schema::positive_int().with_description(
                    "The maximum number of directory entries or text lines to read",
                ),
            ),
        ],
        "read",
    )
}

/// `ReadTool` from `reference/packages/core/src/tool/read.ts:40`.
pub fn def() -> CoreTool {
    tool::make(
        "Read a text file or supported image, page through a large UTF-8 text file by line offset, or list a directory page. Relative paths resolve from the current location; absolute paths inside it are accepted, while external absolute paths require external_directory approval.",
        input(),
        Schema::Raw(serde_json::json!({})),
        None,
        None,
        Some(std::sync::Arc::new(|input, output| to_model_output(input, output))),
        execute,
    )
}

pub fn to_model_output(input: &serde_json::Value, output: &serde_json::Value) -> Vec<Content> {
    if output.get("encoding").and_then(|v| v.as_str()) == Some("base64")
        && output
            .get("mime")
            .and_then(|v| v.as_str())
            .map(|mime| SUPPORTED_IMAGE_MIMES.contains(&mime))
            .unwrap_or(false)
    {
        return vec![
            Content::Text {
                text: "Image read successfully".to_string(),
            },
            Content::File {
                data: output
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                mime: output
                    .get("mime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            },
        ];
    }
    Vec::new()
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
    let offset = input.get("offset").and_then(|v| v.as_i64());
    let limit = input.get("limit").and_then(|v| v.as_i64());

    let target = resolve(&context.location_directory, &path);
    let resource = crate::util::path_relative(&context.location_directory, &target);
    if !crate::util::fs_contains(&context.location_directory, &target) {
        context.assert(crate::core::tool::CorePermissionRequest {
            action: "external_directory".to_string(),
            resources: vec![format!("{}/*", parent_dir(&target))],
            save: None,
            metadata: Some(serde_json::json!({ "filepath": target })),
            source: crate::core::tool::CorePermissionSource {
                message_id: context.assistant_message_id.clone(),
                call_id: context.tool_call_id.clone(),
            },
        })?;
    }
    context.assert(crate::core::tool::CorePermissionRequest {
        action: NAME.to_string(),
        resources: vec![resource.clone()],
        save: Some(vec!["*".to_string()]),
        metadata: None,
        source: crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        },
    })?;

    let kind = read_filesystem::inspect(&target)
        .map_err(|_| ToolError::failure(format!("Unable to read {path}")))?;
    if kind == "directory" {
        let page = read_filesystem::list(&target, &PageInput { offset, limit })
            .map_err(|_| ToolError::failure(format!("Unable to read {path}")))?;
        return serde_json::to_value(page).map_err(|error| ToolError::Other(error.to_string()));
    }
    let value = read_filesystem::read(&target, &resource, &PageInput { offset, limit }).map_err(
        |error| {
            let message = match &error {
                ToolError::Failure(failure) => failure.message.clone(),
                ToolError::Other(message) => message.clone(),
                _ => error.message().to_string(),
            };
            if message.contains("Cannot read binary file")
                || message.contains("Media exceeds")
                || message.contains("out of range")
            {
                error
            } else {
                ToolError::failure(format!("Unable to read {path}"))
            }
        },
    )?;
    Ok(value)
}

fn resolve(location: &str, path: &str) -> String {
    if std::path::Path::new(path).is_absolute() {
        path.to_string()
    } else {
        crate::util::path_resolve(location, path)
    }
}

fn parent_dir(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
