//! Port of `reference/packages/opencode/src/tool/webfetch.ts`.

use crate::model::{ExecuteResult, FilePart, PermissionRequest, ToolContext, ToolError};
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};

const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024;
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
const MAX_TIMEOUT_SECONDS: u64 = 120;

/// `Parameters` from `reference/packages/opencode/src/tool/webfetch.ts:13`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop("url", Schema::string("The URL to fetch content from")),
            opt_prop(
                "format",
                Schema::literals(
                    &["text", "markdown", "html"],
                    "The format to return the content in (text, markdown, or html). Defaults to markdown.",
                )
                .with_default(serde_json::json!("markdown")),
            ),
            opt_prop(
                "timeout",
                Schema::number().with_description("Optional timeout in seconds (max 120)"),
            ),
        ],
        "webfetch",
    )
}

const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

fn accept_header(format: &str) -> &'static str {
    match format {
        "markdown" => "text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1",
        "text" => "text/plain;q=1.0, text/markdown;q=0.9, text/html;q=0.8, */*;q=0.1",
        "html" => {
            "text/html;q=1.0, application/xhtml+xml;q=0.9, text/plain;q=0.8, text/markdown;q=0.7, */*;q=0.1"
        }
        _ => "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
    }
}

/// `isImageAttachment` from `reference/packages/opencode/src/util/media.ts`.
pub fn is_image_attachment(mime: &str) -> bool {
    mime.starts_with("image/") && mime != "image/svg+xml" && mime != "image/vnd.fastbidsheet"
}

/// `WebFetchTool` from `reference/packages/opencode/src/tool/webfetch.ts:24`.
pub fn def() -> crate::tool::tool::Def {
    let raw =
        crate::tool::tool::def_async("webfetch", prompts::WEBFETCH, parameters(), |args, ctx| {
            Box::pin(run(args, ctx))
        });
    crate::tool::tool::wrap("webfetch", raw)
}

async fn run(args: serde_json::Value, ctx: &mut ToolContext) -> Result<ExecuteResult, ToolError> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ToolError::Other(
            "URL must start with http:// or https://".to_string(),
        ));
    }
    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown")
        .to_string();
    let timeout = args.get("timeout").and_then(|v| v.as_f64());

    ctx.ask(PermissionRequest {
        permission: "webfetch".to_string(),
        patterns: vec![url.clone()],
        always: vec!["*".to_string()],
        metadata: serde_json::json!({
            "url": url,
            "format": format,
            "timeout": timeout,
        }),
    })?;

    let timeout_millis = timeout
        .map(|seconds| (seconds * 1000.0) as u64)
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS * 1000)
        .min(MAX_TIMEOUT_SECONDS * 1000);
    let accept = accept_header(&format);
    let headers = [
        ("User-Agent".to_string(), BROWSER_USER_AGENT.to_string()),
        ("Accept".to_string(), accept.to_string()),
        ("Accept-Language".to_string(), "en-US,en;q=0.9".to_string()),
    ];

    let client = crate::http::client();
    let response = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_millis),
        send(&client, &url, &headers),
    )
    .await
    .map_err(|_| ToolError::Other("Request timed out".to_string()))?;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            if error.as_str() == "cloudflare_challenge" {
                let honest_headers = [
                    ("User-Agent".to_string(), "opencode".to_string()),
                    ("Accept".to_string(), accept.to_string()),
                    ("Accept-Language".to_string(), "en-US,en;q=0.9".to_string()),
                ];
                tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_millis),
                    send(&client, &url, &honest_headers),
                )
                .await
                .map_err(|_| ToolError::Other("Request timed out".to_string()))?
                .map_err(|error| ToolError::Other(format!("Unable to fetch {url}: {error}")))?
            } else {
                return Err(ToolError::Other(format!("Unable to fetch {url}: {error}")));
            }
        }
    };

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if let Some(length) = response.content_length() {
        if length as usize > MAX_RESPONSE_SIZE {
            return Err(ToolError::Other(
                "Response too large (exceeds 5MB limit)".to_string(),
            ));
        }
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| ToolError::Other(format!("Unable to fetch {url}: {error}")))?;
    if bytes.len() > MAX_RESPONSE_SIZE {
        return Err(ToolError::Other(
            "Response too large (exceeds 5MB limit)".to_string(),
        ));
    }

    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let title = format!("{url} ({content_type})");

    if is_image_attachment(&mime) {
        let base64 = base64_encode(&bytes);
        let url = format!("data:{mime};base64,{base64}");
        return Ok(ExecuteResult {
            title,
            output: "Image fetched successfully".to_string(),
            metadata: serde_json::json!({}),
            attachments: Some(vec![FilePart {
                mime,
                url,
                filename: None,
            }]),
        });
    }

    let content = String::from_utf8_lossy(&bytes).to_string();
    let output = match format.as_str() {
        "markdown" if content_type.contains("text/html") => convert_html_to_markdown(&content),
        "text" if content_type.contains("text/html") => extract_text_from_html(&content),
        _ => content,
    };

    Ok(ExecuteResult {
        output,
        title,
        metadata: serde_json::json!({}),
        attachments: None,
    })
}

async fn send(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<reqwest::Response, String> {
    let mut request = client.get(url);
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let cf_mitigated = response
        .headers()
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if status == reqwest::StatusCode::FORBIDDEN && cf_mitigated == "challenge" {
        return Err("cloudflare_challenge".to_string());
    }
    if !status.is_success() {
        return Err(format!("HTTP status {status}"));
    }
    Ok(response)
}

/// `extractTextFromHTML` from `reference/packages/opencode/src/tool/webfetch.ts:158`.
pub fn extract_text_from_html(html: &str) -> String {
    let mut text = String::new();
    let mut skip_depth = 0usize;
    let mut current_tag = String::new();
    let mut in_tag = false;
    let mut closing = false;

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let name = current_tag.trim().to_ascii_lowercase();
                let name = name.split_whitespace().next().unwrap_or("").to_string();
                let name = name.trim_start_matches('/').to_string();
                if closing {
                    if skip_depth > 0 {
                        skip_depth -= 1;
                    }
                    closing = false;
                } else if skip_depth > 0
                    || ["script", "style", "noscript", "iframe", "object", "embed"]
                        .contains(&name.as_str())
                {
                    skip_depth += 1;
                }
                current_tag.clear();
            } else {
                if ch == '/' && current_tag.is_empty() {
                    closing = true;
                } else {
                    current_tag.push(ch);
                }
            }
        } else if ch == '<' {
            in_tag = true;
            current_tag.clear();
            closing = false;
        } else if skip_depth == 0 {
            text.push(ch);
        }
    }
    text.trim().to_string()
}

/// `convertHTMLToMarkdown` — simplified TurndownService equivalent.
/// TODO(integration): full Turndown parity for heading/list/link/table rules.
pub fn convert_html_to_markdown(html: &str) -> String {
    let text = extract_text_from_html(html);
    if text.is_empty() {
        return String::new();
    }
    text
}

fn base64_encode(bytes: &[u8]) -> String {
    crate::base64::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::ToolContext;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "format": {
                        "default": "markdown",
                        "description": "The format to return the content in (text, markdown, or html). Defaults to markdown.",
                        "enum": ["text", "markdown", "html"],
                        "type": "string"
                    },
                    "timeout": { "description": "Optional timeout in seconds (max 120)", "type": "number" },
                    "url": { "description": "The URL to fetch content from", "type": "string" }
                },
                "required": ["url"],
                "type": "object"
            })
        );
    }

    #[test]
    fn extracts_text_from_html() {
        let html =
            "<html><head></head><body><script>skip</script><p>Hello <b>world</b></p></body></html>";
        assert_eq!(extract_text_from_html(html), "Hello world");
    }

    #[test]
    fn rejects_non_http_urls() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.path().to_string_lossy().to_string(),
            worktree: dir.path().to_string_lossy().to_string(),
        });
        let result = tokio::runtime::Runtime::new().unwrap().block_on(
            super::def().execute(serde_json::json!({ "url": "file:///etc/passwd" }), &mut ctx),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().message().contains("http://"));
    }
}
