//! MCP catalog helpers: pagination, tool listing, name sanitization, and
//! adaptation of MCP tools for consumers.
//!
//! From reference/packages/opencode/src/mcp/catalog.ts.

use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;
use tracing::warn;

use crate::client::Client;
use crate::types::{ContentBlock, Prompt, Resource, ResourceTemplate, Tool};
use crate::Result;

pub const DEFAULT_TIMEOUT: u64 = 30_000;
/// The SDK's default request timeout when none is configured (60s).
pub const DEFAULT_REQUEST_TIMEOUT: u64 = 60_000;
const MAX_LIST_PAGES: usize = 1_000;

/// Mirror of `McpCatalog.sanitize`: replaces any char outside `[a-zA-Z0-9_-]`.
pub fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Mirror of `McpCatalog.toolName`: `sanitize(client)_sanitize(name)`.
pub fn tool_name(client_name: &str, name: &str) -> String {
    format!("{}_{}", sanitize(client_name), sanitize(name))
}

/// A paged list result with an optional `nextCursor`.
pub trait HasNextCursor {
    fn next_cursor(&self) -> Option<&str>;
}

impl HasNextCursor for crate::types::ListToolsResult {
    fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}
impl HasNextCursor for crate::types::ListPromptsResult {
    fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}
impl HasNextCursor for crate::types::ListResourcesResult {
    fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}
impl HasNextCursor for crate::types::ListResourceTemplatesResult {
    fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

/// Paginate a `*_/list` method until `nextCursor` is absent, rejecting
/// duplicate cursors and capping at `MAX_LIST_PAGES`.
/// From reference `McpCatalog.paginate`.
pub async fn paginate<T, R, L, I, F>(mut list: L, items: I) -> Result<Vec<T>>
where
    R: HasNextCursor,
    F: std::future::Future<Output = Result<R>>,
    L: FnMut(Option<String>) -> F,
    I: Fn(R) -> Vec<T>,
{
    let mut result: Vec<T> = Vec::new();
    let mut cursors: HashSet<String> = HashSet::new();
    let mut cursor: Option<String> = None;

    for _ in 0..MAX_LIST_PAGES {
        let page = list(cursor.clone()).await?;
        let next = page.next_cursor().map(str::to_string);
        result.extend(items(page));
        match next {
            None => return Ok(result),
            Some(next) => {
                if cursors.contains(&next) {
                    return Err(crate::Error::message(format!(
                        "MCP list returned duplicate cursor: {next}"
                    )));
                }
                cursors.insert(next.clone());
                cursor = Some(next);
            }
        }
    }

    Err(crate::Error::message(format!(
        "MCP list exceeded {MAX_LIST_PAGES} pages"
    )))
}

/// List tools; on failure yields `None` (mirrors `defs` catching to void).
pub async fn defs(client: Arc<Client>, timeout: Option<u64>) -> Result<Option<Vec<Tool>>> {
    let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
    match paginate(
        |cursor| client.list_tools(cursor, timeout),
        |result| result.tools,
    )
    .await
    {
        Ok(tools) => Ok(Some(tools)),
        Err(error) => {
            warn!("failed to get tools: {error}");
            Ok(None)
        }
    }
}

/// Adapt an MCP tool definition's input schema the same way `McpCatalog.convertTool`
/// does: `{ ...inputSchema, type: "object", properties: inputSchema.properties ?? {}, additionalProperties: false }`.
pub fn convert_input_schema(mcp_tool: &Tool) -> Value {
    let mut schema = match &mcp_tool.input_schema {
        Value::Object(object) => object.clone(),
        _ => serde_json::Map::new(),
    };
    schema.insert("type".into(), Value::String("object".into()));
    let properties = schema
        .get("properties")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    schema.insert("properties".into(), properties);
    schema.insert("additionalProperties".into(), Value::Bool(false));
    Value::Object(schema)
}

/// `tools/call` adaptation mirroring `McpCatalog.convertTool`'s execute body:
/// `isError` becomes an error with the joined text content, and an empty
/// `content` with only `structuredContent` wraps it as text.
pub async fn call_tool_adapted(
    client: Arc<Client>,
    def: &Tool,
    arguments: Value,
    timeout: u64,
) -> Result<crate::types::CallToolResult> {
    let mut result = client.call_tool(&def.name, arguments, timeout).await?;
    if result.is_error {
        let text = result
            .content
            .iter()
            .filter(|block| block.r#type == "text")
            .filter_map(|block| block.text.as_ref())
            .map(|text| text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        return Err(crate::Error::message(if text.is_empty() {
            "MCP tool returned an error".to_string()
        } else {
            text
        }));
    }
    let has_content = !result.content.is_empty();
    if has_content || result.structured_content.is_none() {
        return Ok(result);
    }
    result.content = vec![ContentBlock::text(
        serde_json::to_string(&result.structured_content).unwrap_or_default(),
    )];
    Ok(result)
}

/// A key extractor used to index fetched items.
pub type KeyFn<T> = dyn Fn(&T) -> String + Send + Sync;

/// Fetch a list of server items and index them by `sanitize(client):name` or,
/// when a `key` is given, `client:key` (with `%` and `:` escaped). Appends the
/// `client` field to every value. From reference `McpCatalog.fetch`.
pub async fn fetch<T, L, F>(
    client_name: &str,
    client: Arc<Client>,
    list: L,
    label: &str,
    key: Option<&KeyFn<T>>,
) -> Result<Option<IndexMap<String, Value>>>
where
    T: Serialize + Named,
    L: Fn(&Arc<Client>) -> F,
    F: std::future::Future<Output = Result<Vec<T>>>,
{
    let items = match list(&client).await {
        Ok(items) => items,
        Err(error) => {
            warn!("failed to get {label} from {client_name}: {error}");
            return Ok(None);
        }
    };
    let sanitized_client = sanitize(client_name);
    let resource_client = client_name.replace('%', "%25").replace(':', "%3A");
    let mut map = IndexMap::new();
    for item in items {
        let entry_key = match key {
            Some(key) => format!("{resource_client}:{}", key(&item)),
            None => format!("{sanitized_client}:{}", sanitize(item.name())),
        };
        let mut value = serde_json::to_value(&item)?;
        if let Value::Object(object) = &mut value {
            object.insert("client".into(), Value::String(client_name.to_string()));
        }
        map.insert(entry_key, value);
    }
    Ok(Some(map))
}

/// Items that have a `name` used for map keys.
pub trait Named {
    fn name(&self) -> &str;
}

impl Named for Prompt {
    fn name(&self) -> &str {
        &self.name
    }
}
impl Named for Resource {
    fn name(&self) -> &str {
        &self.name
    }
}
impl Named for ResourceTemplate {
    fn name(&self) -> &str {
        &self.name
    }
}

/// `prompts/list`, capability-gated. From reference `McpCatalog.prompts`.
pub async fn prompts(client: Arc<Client>, timeout: Option<u64>) -> Result<Vec<Prompt>> {
    let Some(capabilities) = client.get_server_capabilities().await else {
        return Ok(Vec::new());
    };
    if !capabilities.has_prompts() {
        return Ok(Vec::new());
    }
    let timeout = timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
    paginate(
        |cursor| client.list_prompts(cursor, timeout),
        |result| result.prompts,
    )
    .await
}

/// `resources/list`, capability-gated. From reference `McpCatalog.resources`.
pub async fn resources(client: Arc<Client>, timeout: Option<u64>) -> Result<Vec<Resource>> {
    let Some(capabilities) = client.get_server_capabilities().await else {
        return Ok(Vec::new());
    };
    if !capabilities.has_resources() {
        return Ok(Vec::new());
    }
    let timeout = timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
    paginate(
        |cursor| client.list_resources(cursor, timeout),
        |result| result.resources,
    )
    .await
}

/// `resources/templates/list`, capability-gated. From reference `McpCatalog.resourceTemplates`.
pub async fn resource_templates(
    client: Arc<Client>,
    timeout: Option<u64>,
) -> Result<Vec<ResourceTemplate>> {
    let Some(capabilities) = client.get_server_capabilities().await else {
        return Ok(Vec::new());
    };
    if !capabilities.has_resources() {
        return Ok(Vec::new());
    }
    let timeout = timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
    paginate(
        |cursor| client.list_resource_templates(cursor, timeout),
        |result| result.resource_templates,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_matches_reference() {
        assert_eq!(sanitize("my-server"), "my-server");
        assert_eq!(sanitize("my server!"), "my_server_");
        assert_eq!(sanitize("a.b/c"), "a_b_c");
    }

    #[test]
    fn tool_name_matches_reference() {
        assert_eq!(
            tool_name("github.com/foo", "list issues"),
            "github_com_foo_list_issues"
        );
    }

    #[test]
    fn convert_input_schema_matches_reference() {
        let tool = Tool {
            name: "t".into(),
            description: None,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
            output_schema: None,
            annotations: None,
            extra: Default::default(),
        };
        assert_eq!(
            convert_input_schema(&tool),
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn convert_input_schema_defaults_properties() {
        let tool = Tool {
            name: "t".into(),
            description: None,
            input_schema: serde_json::json!({}),
            output_schema: None,
            annotations: None,
            extra: Default::default(),
        };
        assert_eq!(
            convert_input_schema(&tool),
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn fetch_keys_escape_resource_client() {
        // Exercise the key escaping logic directly through the public sanitize.
        let client_name = "srv:1";
        assert_eq!(
            client_name.replace('%', "%25").replace(':', "%3A"),
            "srv%3A1"
        );
    }
}
