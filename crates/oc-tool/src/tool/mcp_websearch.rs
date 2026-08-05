//! Port of `reference/packages/opencode/src/tool/mcp-websearch.ts`.
//!
//! MCP `tools/call` HTTP client for the Exa / Parallel web search backends.

use serde_json::Value as JsonValue;

pub const EXA_URL: &str = "https://mcp.exa.ai/mcp";
pub const PARALLEL_URL: &str = "https://search.parallel.ai/mcp";

const TOOL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// Exa `web_search_exa` arguments.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchArgs {
    pub query: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "numResults")]
    pub num_results: i64,
    pub livecrawl: String,
    #[serde(
        rename = "contextMaxCharacters",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_max_characters: Option<i64>,
}

/// Parallel `web_search` arguments.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParallelSearchArgs {
    pub objective: String,
    #[serde(rename = "search_queries")]
    pub search_queries: Vec<String>,
    #[serde(rename = "session_id", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(rename = "model_name", skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct McpResult {
    result: McpResultBody,
}

#[derive(Debug, serde::Deserialize)]
struct McpResultBody {
    content: Vec<McpContent>,
}

#[derive(Debug, serde::Deserialize)]
struct McpContent {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    kind: String,
    text: String,
}

fn parse_payload(payload: &str) -> Option<String> {
    let trimmed = payload.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let parsed: McpResult = serde_json::from_str(trimmed).ok()?;
    parsed
        .result
        .content
        .into_iter()
        .find(|item| !item.text.is_empty())
        .map(|item| item.text)
}

/// `parseResponse` from `reference/packages/opencode/src/tool/mcp-websearch.ts:30`.
pub fn parse_response(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if let Some(direct) = parse_payload(trimmed) {
        return Some(direct);
    }
    for line in body.split('\n') {
        if let Some(payload) = line.strip_prefix("data: ") {
            if let Some(data) = parse_payload(payload) {
                return Some(data);
            }
        }
    }
    None
}

fn exa_url() -> String {
    match std::env::var("EXA_API_KEY") {
        Ok(key) => {
            let encoded = urlencoding(&key);
            format!("{EXA_URL}?exaApiKey={encoded}")
        }
        Err(_) => EXA_URL.to_string(),
    }
}

fn urlencoding(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `McpWebSearch.call` from `reference/packages/opencode/src/tool/mcp-websearch.ts:69`.
pub async fn call(
    client: &reqwest::Client,
    url: &str,
    tool: &str,
    value: JsonValue,
    headers: Vec<(String, String)>,
) -> Result<Option<String>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": value },
    });
    let mut request = client
        .post(url)
        .header("Accept", "application/json, text/event-stream");
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = tokio::time::timeout(TOOL_TIMEOUT, request.json(&body).send())
        .await
        .map_err(|_| format!("{tool} request timed out"))?
        .map_err(|error| format!("{tool} request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "{tool} request failed with status {}",
            response.status()
        ));
    }
    let text = tokio::time::timeout(TOOL_TIMEOUT, response.text())
        .await
        .map_err(|_| format!("{tool} request timed out"))?
        .map_err(|error| format!("{tool} request failed: {error}"))?;
    Ok(parse_response(&text))
}

/// Exa provider URL (respects `EXA_API_KEY`).
pub fn exa_endpoint() -> String {
    exa_url()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_direct_json_payload() {
        let body = r#"{"result":{"content":[{"type":"text","text":"hello"}]}}"#;
        assert_eq!(parse_response(body).as_deref(), Some("hello"));
    }

    #[test]
    fn parses_sse_payload() {
        let body = "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"found\"}]}}\n\n";
        assert_eq!(parse_response(body).as_deref(), Some("found"));
    }

    #[test]
    fn returns_none_for_non_json() {
        assert_eq!(parse_response("hello"), None);
    }
}
