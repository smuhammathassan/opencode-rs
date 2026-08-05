//! Tool call update builders.
//!
//! From reference/packages/opencode/src/acp/tool.ts. Pure functions that turn
//! opencode tool part states into ACP tool call notifications.

use serde_json::{Map, Value};

use crate::sdk::{ToolStateCompleted, ToolStateError, ToolStateRunning};
use crate::types::{
    ContentBlock, Diff, ToolCall, ToolCallContent, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolKind,
};

/// Raw tool input parameters.
pub type ToolInput = Map<String, Value>;

/// A tool attachment (usually an opencode `FilePart`).
#[derive(Debug, Clone, Default)]
pub struct ToolAttachment {
    pub mime: Option<String>,
    pub url: Option<String>,
}

impl From<&crate::sdk::FilePart> for ToolAttachment {
    fn from(part: &crate::sdk::FilePart) -> Self {
        Self {
            mime: Some(part.mime.clone()),
            url: Some(part.url.clone()),
        }
    }
}

/// `toToolKind` from reference/packages/opencode/src/acp/tool.ts.
pub fn to_tool_kind(tool_name: &str) -> ToolKind {
    match tool_name.to_lowercase().as_str() {
        "bash" | "shell" => ToolKind::Execute,
        "webfetch" => ToolKind::Fetch,
        "edit" | "apply_patch" | "patch" | "write" => ToolKind::Edit,
        "grep"
        | "glob"
        | "context"
        | "context7_resolve_library_id"
        | "context7_get_library_docs" => ToolKind::Search,
        "read" => ToolKind::Read,
        "task" => ToolKind::Think,
        _ => ToolKind::Other,
    }
}

/// `toLocations` from reference/packages/opencode/src/acp/tool.ts.
pub fn to_locations(
    tool_name: &str,
    input: &ToolInput,
    cwd: Option<&str>,
) -> Vec<ToolCallLocation> {
    match tool_name.to_lowercase().as_str() {
        "bash" | "shell" => match shell_workdir(input, cwd) {
            Some(workdir) => vec![ToolCallLocation { path: workdir }],
            None => Vec::new(),
        },
        "read" | "edit" | "write" => location_from([
            string_value(input.get("filePath")).or_else(|| string_value(input.get("filepath")))
        ]),
        "external_directory" => location_from([
            string_value(input.get("filePath")).or_else(|| string_value(input.get("filepath"))),
            string_value(input.get("parentDir")),
            string_value(input.get("directories")),
        ]),
        "grep"
        | "glob"
        | "context"
        | "context7_resolve_library_id"
        | "context7_get_library_docs" => location_from([string_value(input.get("path"))]),
        _ => Vec::new(),
    }
}

/// `completedToolContent` from reference/packages/opencode/src/acp/tool.ts.
pub fn completed_tool_content(tool_name: &str, state: &ToolStateCompleted) -> Vec<ToolCallContent> {
    let text = if tool_name.to_lowercase() == "read" {
        read_display_text(state.metadata.get("display")).unwrap_or_else(|| state.output.clone())
    } else {
        state.output.clone()
    };
    let mut content: Vec<ToolCallContent> = vec![ToolCallContent::Content {
        content: ContentBlock::Text(crate::types::TextContent {
            text,
            annotations: None,
        }),
    }];

    if to_tool_kind(tool_name) == ToolKind::Edit {
        content.extend(diff_content(&state.input));
    }

    let attachments = state
        .attachments
        .as_ref()
        .map(|items| items.iter().map(ToolAttachment::from).collect::<Vec<_>>())
        .unwrap_or_default();
    content.extend(image_contents(&attachments));
    content
}

/// `pendingToolCall` from reference/packages/opencode/src/acp/tool.ts.
pub fn pending_tool_call(input: PendingToolCallInput) -> ToolCall {
    ToolCall {
        tool_call_id: input.tool_call_id,
        title: tool_title(
            input.tool_name,
            &input.state.input,
            input.state.title.as_deref(),
        ),
        kind: to_tool_kind(input.tool_name),
        status: ToolCallStatus::Pending,
        locations: to_locations(input.tool_name, &input.state.input, input.cwd),
        raw_input: raw_input(input.tool_name, &input.state.input, input.cwd),
    }
}

/// Input to [`pending_tool_call`].
pub struct PendingToolCallInput<'a> {
    pub tool_call_id: String,
    pub tool_name: &'a str,
    pub state: RunningToolInput,
    pub cwd: Option<&'a str>,
}

/// The input state consumed by the running/pending builders.
pub struct RunningToolInput {
    pub input: ToolInput,
    pub title: Option<String>,
}

/// `runningToolUpdate` from reference/packages/opencode/src/acp/tool.ts.
pub fn running_tool_update(input: RunningToolUpdateInput) -> ToolCallUpdate {
    let content = input.output.map(|output| {
        vec![ToolCallContent::Content {
            content: ContentBlock::Text(crate::types::TextContent {
                text: output,
                annotations: None,
            }),
        }]
    });
    ToolCallUpdate {
        tool_call_id: input.tool_call_id,
        status: ToolCallStatus::InProgress,
        kind: Some(to_tool_kind(input.tool_name)),
        title: Some(tool_title(
            input.tool_name,
            &input.state.input,
            input.state.title.as_deref(),
        )),
        locations: Some(to_locations(input.tool_name, &input.state.input, input.cwd)),
        raw_input: Some(raw_input(input.tool_name, &input.state.input, input.cwd)),
        content,
        raw_output: None,
    }
}

/// Input to [`running_tool_update`].
pub struct RunningToolUpdateInput<'a> {
    pub tool_call_id: String,
    pub tool_name: &'a str,
    pub state: RunningToolInput,
    pub output: Option<String>,
    pub cwd: Option<&'a str>,
}

/// `duplicateRunningToolUpdate` from reference/packages/opencode/src/acp/tool.ts.
pub fn duplicate_running_tool_update(input: RunningToolUpdateInput) -> ToolCallUpdate {
    ToolCallUpdate {
        tool_call_id: input.tool_call_id,
        status: ToolCallStatus::InProgress,
        kind: Some(to_tool_kind(input.tool_name)),
        title: Some(tool_title(
            input.tool_name,
            &input.state.input,
            input.state.title.as_deref(),
        )),
        locations: Some(to_locations(input.tool_name, &input.state.input, input.cwd)),
        raw_input: Some(raw_input(input.tool_name, &input.state.input, input.cwd)),
        content: None,
        raw_output: None,
    }
}

/// `completedToolUpdate` from reference/packages/opencode/src/acp/tool.ts.
pub fn completed_tool_update(input: CompletedToolUpdateInput) -> ToolCallUpdate {
    ToolCallUpdate {
        tool_call_id: input.tool_call_id,
        status: ToolCallStatus::Completed,
        kind: None,
        title: if input.state.title.is_empty() {
            None
        } else {
            Some(input.state.title.clone())
        },
        locations: None,
        raw_input: None,
        content: Some(completed_tool_content(input.tool_name, &input.state)),
        raw_output: Some(completed_tool_raw_output(&input.state)),
    }
}

/// Input to [`completed_tool_update`].
pub struct CompletedToolUpdateInput<'a> {
    pub tool_call_id: String,
    pub tool_name: &'a str,
    pub state: ToolStateCompleted,
    pub cwd: Option<&'a str>,
}

/// `errorToolUpdate` from reference/packages/opencode/src/acp/tool.ts.
pub fn error_tool_update(input: ErrorToolUpdateInput) -> ToolCallUpdate {
    let mut raw_output = Map::new();
    raw_output.insert("error".into(), Value::String(input.state.error.clone()));
    if let Some(metadata) = &input.state.metadata {
        raw_output.insert("metadata".into(), Value::Object(metadata.clone()));
    }
    ToolCallUpdate {
        tool_call_id: input.tool_call_id,
        status: ToolCallStatus::Failed,
        kind: Some(to_tool_kind(input.tool_name)),
        title: Some(tool_title(input.tool_name, &input.state.input, None)),
        locations: Some(to_locations(input.tool_name, &input.state.input, input.cwd)),
        raw_input: Some(raw_input(input.tool_name, &input.state.input, input.cwd)),
        content: Some(vec![ToolCallContent::Content {
            content: ContentBlock::Text(crate::types::TextContent {
                text: input.state.error.clone(),
                annotations: None,
            }),
        }]),
        raw_output: Some(Value::Object(raw_output)),
    }
}

/// Input to [`error_tool_update`].
pub struct ErrorToolUpdateInput<'a> {
    pub tool_call_id: String,
    pub tool_name: &'a str,
    pub state: ToolStateError,
    pub cwd: Option<&'a str>,
}

/// `completedToolRawOutput` from reference/packages/opencode/src/acp/tool.ts.
pub fn completed_tool_raw_output(state: &ToolStateCompleted) -> Value {
    let mut output = Map::new();
    output.insert("output".into(), Value::String(state.output.clone()));
    if !state.metadata.is_empty() {
        output.insert("metadata".into(), Value::Object(state.metadata.clone()));
    }
    if let Some(attachments) = &state.attachments {
        if !attachments.is_empty() {
            let attachments: Vec<Value> = attachments
                .iter()
                .map(|attachment| serde_json::to_value(attachment).unwrap_or(Value::Null))
                .collect();
            output.insert("attachments".into(), Value::Array(attachments));
        }
    }
    Value::Object(output)
}

/// `imageContents` from reference/packages/opencode/src/acp/tool.ts.
pub fn image_contents(attachments: &[ToolAttachment]) -> Vec<ToolCallContent> {
    extract_image_attachments(attachments)
        .into_iter()
        .map(|image| ToolCallContent::Content {
            content: ContentBlock::Image(crate::types::ImageContent {
                mime_type: Some(image.mime_type),
                data: Some(image.data),
                uri: None,
                annotations: None,
            }),
        })
        .collect()
}

/// `extractImageAttachments` from reference/packages/opencode/src/acp/tool.ts.
pub fn extract_image_attachments(attachments: &[ToolAttachment]) -> Vec<ImageAttachment> {
    attachments.iter().filter_map(data_url_image).collect()
}

/// `shellOutputSnapshot` from reference/packages/opencode/src/acp/tool.ts.
pub fn shell_output_snapshot(state: &ToolStateRunning) -> Option<String> {
    state
        .metadata
        .as_ref()
        .and_then(|metadata| string_value(metadata.get("output")))
}

/// An image attachment extracted from tool output.
pub struct ImageAttachment {
    pub mime_type: String,
    pub data: String,
}

/// `diffContent` from reference/packages/opencode/src/acp/tool.ts.
fn diff_content(input: &ToolInput) -> Vec<ToolCallContent> {
    let old_text = string_value(input.get("oldString"));
    let new_text =
        string_value(input.get("newString")).or_else(|| string_value(input.get("content")));
    let (Some(old_text), Some(new_text)) = (old_text, new_text) else {
        return Vec::new();
    };
    vec![ToolCallContent::Diff(Diff {
        path: string_value(input.get("filePath")).unwrap_or_default(),
        old_text: Some(old_text),
        new_text,
    })]
}

/// `readDisplayText` from reference/packages/opencode/src/acp/tool.ts.
fn read_display_text(display: Option<&Value>) -> Option<String> {
    let display = display?;
    let info = display.as_object()?;
    match info.get("type").and_then(Value::as_str) {
        Some("file") => string_value(info.get("text")),
        Some("directory") => {
            let entries = info.get("entries")?.as_array()?;
            let mut lines = Vec::new();
            for entry in entries {
                if let Some(line) = entry.as_str() {
                    lines.push(line.to_string());
                }
            }
            Some(lines.join("\n"))
        }
        _ => None,
    }
}

/// `dataUrlImage` from reference/packages/opencode/src/acp/tool.ts.
fn data_url_image(attachment: &ToolAttachment) -> Option<ImageAttachment> {
    let url = attachment.url.as_deref()?;
    let regex = regex_capture_data_url(url);
    let mime = regex
        .as_ref()
        .and_then(|(mime, _)| mime.clone())
        .or_else(|| attachment.mime.clone())?;
    if !mime.starts_with("image/") {
        return None;
    }
    let data = regex.and_then(|(_, data)| data)?;
    Some(ImageAttachment {
        mime_type: mime,
        data,
    })
}

/// Extract `(mime, base64)` from `data:([^;,]+)(?:;[^,]*)*;base64,(.*)`.
fn regex_capture_data_url(url: &str) -> Option<(Option<String>, Option<String>)> {
    let rest = url.strip_prefix("data:")?;
    let index = rest.find(";base64,")?;
    let mime_and_params = &rest[..index];
    let data = rest[index + ";base64,".len()..].to_string();
    let mime = mime_and_params.split(';').next().unwrap_or("");
    if mime.is_empty() {
        return Some((None, Some(data)));
    }
    Some((Some(mime.to_string()), Some(data)))
}

/// `toolTitle` from reference/packages/opencode/src/acp/tool.ts.
fn tool_title(tool_name: &str, input: &ToolInput, fallback: Option<&str>) -> String {
    if is_shell(tool_name) {
        let command = shell_command(input)
            .or_else(|| fallback.map(str::to_string))
            .unwrap_or_else(|| tool_name.to_string());
        return command;
    }
    fallback
        .map(str::to_string)
        .unwrap_or_else(|| tool_name.to_string())
}

/// `rawInput` from reference/packages/opencode/src/acp/tool.ts.
fn raw_input(tool_name: &str, input: &ToolInput, cwd: Option<&str>) -> Value {
    if !is_shell(tool_name) {
        return Value::Object(input.clone());
    }
    if input.contains_key("cwd") || input.contains_key("workdir") {
        return Value::Object(input.clone());
    }
    match shell_workdir(input, cwd) {
        Some(workdir) => {
            let mut next = input.clone();
            next.insert("cwd".into(), Value::String(workdir));
            Value::Object(next)
        }
        None => Value::Object(input.clone()),
    }
}

/// `shellWorkdir` from reference/packages/opencode/src/acp/tool.ts.
fn shell_workdir(input: &ToolInput, cwd: Option<&str>) -> Option<String> {
    let explicit = string_value(input.get("workdir")).or_else(|| string_value(input.get("cwd")));
    resolve_path(explicit.as_deref(), cwd).or_else(|| cwd.map(str::to_string))
}

/// `resolvePath` from reference/packages/opencode/src/acp/tool.ts.
fn resolve_path(value: Option<&str>, cwd: Option<&str>) -> Option<String> {
    let value = value?;
    if std::path::Path::new(value).is_absolute() {
        return Some(value.to_string());
    }
    let base = cwd.unwrap_or(".");
    Some(
        std::path::Path::new(base)
            .join(value)
            .to_string_lossy()
            .into_owned(),
    )
}

/// `shellCommand` from reference/packages/opencode/src/acp/tool.ts.
fn shell_command(input: &ToolInput) -> Option<String> {
    string_value(input.get("command")).or_else(|| string_value(input.get("cmd")))
}

fn is_shell(tool_name: &str) -> bool {
    let tool = tool_name.to_lowercase();
    tool == "bash" || tool == "shell"
}

/// `locationFrom` from reference/packages/opencode/src/acp/tool.ts.
fn location_from(values: impl IntoIterator<Item = Option<String>>) -> Vec<ToolCallLocation> {
    let mut seen = std::collections::HashSet::new();
    let mut locations = Vec::new();
    for value in values.into_iter().flatten() {
        if seen.insert(value.clone()) {
            locations.push(ToolCallLocation { path: value });
        }
    }
    locations
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// Convert an SDK tool part state into a builder-compatible running state.
pub fn running_state_from_tool_state(state: &ToolStateRunning) -> RunningToolInput {
    RunningToolInput {
        input: state.input.clone(),
        title: state.title.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_kind_mapping() {
        assert_eq!(to_tool_kind("bash"), ToolKind::Execute);
        assert_eq!(to_tool_kind("webfetch"), ToolKind::Fetch);
        assert_eq!(to_tool_kind("apply_patch"), ToolKind::Edit);
        assert_eq!(to_tool_kind("grep"), ToolKind::Search);
        assert_eq!(to_tool_kind("read"), ToolKind::Read);
        assert_eq!(to_tool_kind("task"), ToolKind::Think);
        assert_eq!(to_tool_kind("unknown"), ToolKind::Other);
    }

    #[test]
    fn shell_workdir_resolution() {
        let mut input = ToolInput::new();
        input.insert("command".into(), json!("ls"));
        let locations = to_locations("bash", &input, Some("/cwd"));
        assert_eq!(
            locations,
            vec![ToolCallLocation {
                path: "/cwd".into()
            }]
        );

        let pending = pending_tool_call(PendingToolCallInput {
            tool_call_id: "t1".into(),
            tool_name: "bash",
            state: RunningToolInput { input, title: None },
            cwd: Some("/cwd"),
        });
        assert_eq!(pending.raw_input, json!({ "command": "ls", "cwd": "/cwd" }));
    }

    #[test]
    fn completed_tool_content_with_diff() {
        let mut input = ToolInput::new();
        input.insert("oldString".into(), json!("a"));
        input.insert("newString".into(), json!("b"));
        input.insert("filePath".into(), json!("/x.rs"));
        let state = ToolStateCompleted {
            input,
            output: "done".into(),
            title: "edit".into(),
            metadata: Map::new(),
            attachments: None,
        };
        let content = completed_tool_content("edit", &state);
        assert_eq!(
            serde_json::to_value(&content).unwrap(),
            json!([
                { "type": "content", "content": { "type": "text", "text": "done" } },
                { "type": "diff", "path": "/x.rs", "oldText": "a", "newText": "b" }
            ])
        );
    }
}
