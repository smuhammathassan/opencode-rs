/// From reference/packages/opencode/src/session/tools.ts
///
/// Pure helpers for the MCP resource tools (`list_mcp_resources`,
/// `list_mcp_resource_templates`, `read_mcp_resource`) that format resource
/// listings and content for the model.
///
/// TODO(integration): the `SessionTools.resolve` tool registration depends on
/// the oc-tool registry + oc-mcp clients; only the pure formatting logic is
/// ported here.
use crate::v1::FilePart;

pub const MCP_RESOURCE_TOOLS_LIST: &str = "list_mcp_resources";
pub const MCP_RESOURCE_TOOLS_LIST_TEMPLATES: &str = "list_mcp_resource_templates";
pub const MCP_RESOURCE_TOOLS_READ: &str = "read_mcp_resource";

pub const MAX_MCP_RESOURCE_BLOB_BYTES: usize = 10 * 1024 * 1024;

pub const SUPPORTED_MCP_RESOURCE_ATTACHMENT_MIMES: [&str; 5] = [
    "application/pdf",
    "image/gif",
    "image/jpeg",
    "image/png",
    "image/webp",
];

/// From reference `tools.ts:toRecord`.
pub fn to_record(value: &serde_json::Value) -> crate::JsonMap {
    match value {
        serde_json::Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => crate::JsonMap::new(),
    }
}

/// From reference `tools.ts:optionalString`.
pub fn optional_string(args: &crate::JsonMap, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) if value.is_empty() => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(format!("{key} must be a string")),
    }
}

/// From reference `tools.ts:requiredString`.
pub fn required_string(args: &crate::JsonMap, key: &str) -> Result<String, String> {
    optional_string(args, key)?.ok_or_else(|| format!("{key} is required"))
}

/// From reference `tools.ts:parseListMcpResourcesArgs`.
pub fn parse_list_mcp_resources_args(value: &serde_json::Value) -> Result<Option<String>, String> {
    let args = to_record(value);
    optional_string(&args, "server")
}

/// From reference `tools.ts:parseReadMcpResourceArgs`.
pub fn parse_read_mcp_resource_args(value: &serde_json::Value) -> Result<(String, String), String> {
    let args = to_record(value);
    Ok((
        required_string(&args, "server")?,
        required_string(&args, "uri")?,
    ))
}

/// From reference `tools.ts:formatMcpResource` — strip `client`, add `server`.
pub fn format_mcp_resource(resource: &crate::JsonMap, client: &str) -> crate::JsonMap {
    let mut result: crate::JsonMap = resource
        .iter()
        .filter(|(key, _)| key.as_str() != "client")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    result.insert(
        "server".to_string(),
        serde_json::Value::String(client.to_string()),
    );
    result
}

/// From reference `tools.ts:formatMcpResourceTemplate`.
pub fn format_mcp_resource_template(template: &crate::JsonMap, client: &str) -> crate::JsonMap {
    format_mcp_resource(template, client)
}

/// From reference `tools.ts:base64Size`.
pub fn base64_size(value: &str) -> usize {
    let trimmed: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    let padding = if trimmed.ends_with("==") {
        2
    } else if trimmed.ends_with('=') {
        1
    } else {
        0
    };
    ((trimmed.len() * 3) / 4).saturating_sub(padding)
}

/// From reference `tools.ts:formatBytes`.
pub fn format_bytes(value: usize) -> String {
    if value < 1024 {
        format!("{value} B")
    } else if value < 1024 * 1024 {
        format!("{} KB", (value as f64 / 1024.0).ceil())
    } else {
        format!("{} MB", (value as f64 / (1024.0 * 1024.0)).ceil())
    }
}

/// From reference `tools.ts:formatMcpResourceContent` — flatten resource
/// contents into text and file attachments.
pub fn format_mcp_resource_content(
    server: &str,
    uri: &str,
    contents: &serde_json::Value,
) -> McpResourceContent {
    let items: Vec<&serde_json::Value> = match contents.get("contents") {
        Some(serde_json::Value::Array(items)) => items.iter().collect(),
        Some(item) => vec![item],
        None => vec![],
    };
    let item_count = items.len();
    let mut text: Vec<String> = Vec::new();
    let mut attachments: Vec<FilePart> = Vec::new();

    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let item_uri = object
            .get("uri")
            .and_then(|v| v.as_str())
            .unwrap_or(uri)
            .to_string();
        let mime = object
            .get("mimeType")
            .and_then(|v| v.as_str())
            .unwrap_or("application/octet-stream")
            .to_string();
        if let Some(text_value) = object.get("text").and_then(|v| v.as_str()) {
            text.push(format!("Resource: {item_uri}\nMIME: {mime}\n{text_value}"));
            continue;
        }
        if let Some(blob) = object.get("blob").and_then(|v| v.as_str()) {
            let size = base64_size(blob);
            if !SUPPORTED_MCP_RESOURCE_ATTACHMENT_MIMES.contains(&mime.as_str()) {
                text.push(format!(
                    "[Binary MCP resource omitted: {item_uri} ({mime}, {}) is not a supported attachment type]",
                    format_bytes(size)
                ));
                continue;
            }
            if size > MAX_MCP_RESOURCE_BLOB_BYTES {
                text.push(format!(
                    "[Binary MCP resource omitted: {item_uri} ({mime}, {}) exceeds {}]",
                    format_bytes(size),
                    format_bytes(MAX_MCP_RESOURCE_BLOB_BYTES)
                ));
                continue;
            }
            text.push(format!(
                "[Binary MCP resource attached: {item_uri} ({mime})]"
            ));
            attachments.push(FilePart {
                base: crate::v1::PartBase {
                    id: crate::schema::create_part(None),
                    session_id: String::new(),
                    message_id: String::new(),
                },
                type_: "file".into(),
                mime,
                filename: Some(item_uri),
                url: format!(
                    "data:{};base64,{blob}",
                    object
                        .get("mimeType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("application/octet-stream")
                ),
                source: None,
            });
            continue;
        }
        text.push(format!(
            "[MCP resource content without text or blob: {item_uri}]"
        ));
    }

    McpResourceContent {
        contents: item_count,
        attachments,
        text: if text.is_empty() {
            format!("MCP resource {uri} from {server} returned no contents.")
        } else {
            text.join("\n\n")
        },
    }
}

#[derive(Debug, Clone)]
pub struct McpResourceContent {
    pub contents: usize,
    pub attachments: Vec<FilePart>,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_size_matches_reference() {
        assert_eq!(base64_size("aGVsbG8="), 5);
        assert_eq!(format_bytes(2048), "2 KB");
    }

    #[test]
    fn read_args_required_fields() {
        let value = serde_json::json!({ "server": "s", "uri": "u" });
        let (server, uri) = parse_read_mcp_resource_args(&value).unwrap();
        assert_eq!(server, "s");
        assert_eq!(uri, "u");
        assert!(parse_read_mcp_resource_args(&serde_json::json!({ "server": "s" })).is_err());
    }

    #[test]
    fn format_mcp_resource_swaps_client_for_server() {
        let mut resource = crate::JsonMap::new();
        resource.insert("uri".into(), serde_json::json!("file:///a"));
        resource.insert("client".into(), serde_json::json!("cli"));
        let formatted = format_mcp_resource(&resource, "cli");
        assert!(formatted.get("client").is_none());
        assert_eq!(formatted.get("server").unwrap(), "cli");
    }

    #[test]
    fn format_content_flattens_text_and_attachments() {
        let contents = serde_json::json!({
            "contents": [
                { "uri": "file:///a.txt", "mimeType": "text/plain", "text": "hello" },
                { "uri": "file:///a.png", "mimeType": "image/png", "blob": "iVBORw0KGgo=" }
            ]
        });
        let result = format_mcp_resource_content("cli", "file:///a.txt", &contents);
        assert_eq!(result.contents, 2);
        assert!(result.text.contains("Resource: file:///a.txt"));
        assert_eq!(result.attachments.len(), 1);
    }
}
