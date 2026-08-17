//! Port of `reference/packages/opencode/src/tool/websearch.ts`.

use crate::checksum::checksum;
use crate::model::{ExecuteResult, PermissionRequest, ToolContext, ToolError};
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::mcp_websearch;

const INSTALLATION_VERSION: &str = "local";

/// `Parameters` from `reference/packages/opencode/src/tool/websearch.ts:10`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop("query", Schema::string("Websearch query")),
            opt_prop(
                "numResults",
                Schema::number().with_description("Number of search results to return (default: 8)"),
            ),
            opt_prop(
                "livecrawl",
                Schema::literals(
                    &["fallback", "preferred"],
                    "Live crawl mode - 'fallback': use live crawling as backup if cached content unavailable, 'preferred': prioritize live crawling (default: 'fallback')",
                ),
            ),
            opt_prop(
                "type",
                Schema::literals(
                    &["auto", "fast", "deep"],
                    "Search type - 'auto': balanced search (default), 'fast': quick results, 'deep': comprehensive search",
                ),
            ),
            opt_prop(
                "contextMaxCharacters",
                Schema::number().with_description("Maximum characters for context string optimized for LLMs (default: 10000)"),
            ),
        ],
        "websearch",
    )
}

/// `selectWebSearchProvider` from `reference/packages/opencode/src/tool/websearch.ts:30`.
pub fn select_web_search_provider(session_id: &str, exa: bool, parallel: bool) -> &'static str {
    match std::env::var("OPENCODE_WEBSEARCH_PROVIDER").as_deref() {
        Ok("exa") => return "exa",
        Ok("parallel") => return "parallel",
        _ => {}
    }
    if parallel {
        return "parallel";
    }
    if exa {
        return "exa";
    }
    match checksum(session_id).and_then(|value| u64::from_str_radix(&value, 36).ok()) {
        Some(parsed) if parsed % 2 == 0 => "exa",
        _ => "parallel",
    }
}

/// `webSearchProviderLabel` from `reference/packages/opencode/src/tool/websearch.ts:39`.
pub fn web_search_provider_label(provider: &str) -> &'static str {
    match provider {
        "parallel" => "Parallel Web Search",
        "exa" => "Exa Web Search",
        _ => "Web Search",
    }
}

/// `webSearchModelName` from `reference/packages/opencode/src/tool/websearch.ts:45`.
pub fn web_search_model_name(extra: &serde_json::Value) -> Option<String> {
    let model = extra.get("model")?;
    let api_id = model
        .get("api")
        .and_then(|api| api.get("id"))
        .and_then(|id| id.as_str());
    let id = model.get("id").and_then(|id| id.as_str());
    let name = api_id.or(id)?;
    let truncated: String = name.chars().take(100).collect();
    Some(truncated)
}

/// `parallelAuthHeaders` from `reference/packages/opencode/src/tool/websearch.ts:54`.
pub fn parallel_auth_headers() -> Vec<(String, String)> {
    let mut headers = vec![(
        "User-Agent".to_string(),
        format!("opencode/{INSTALLATION_VERSION}"),
    )];
    if let Ok(key) = std::env::var("PARALLEL_API_KEY") {
        headers.push(("Authorization".to_string(), format!("Bearer {key}")));
    }
    headers
}

/// `WebSearchTool` from `reference/packages/opencode/src/tool/websearch.ts:99`.
pub fn def(exa: bool, parallel: bool) -> crate::tool::tool::Def {
    let description = prompts::WEBSEARCH.replace("{{year}}", &current_year().to_string());
    let raw =
        crate::tool::tool::def_async("websearch", description, parameters(), move |args, ctx| {
            Box::pin(run(args, ctx, exa, parallel))
        });
    crate::tool::tool::wrap("websearch", raw)
}

async fn run(
    args: serde_json::Value,
    ctx: &mut ToolContext,
    exa: bool,
    parallel: bool,
) -> Result<ExecuteResult, ToolError> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let num_results = args
        .get("numResults")
        .and_then(|v| v.as_f64())
        .map(|v| v as i64);
    let livecrawl = args
        .get("livecrawl")
        .and_then(|v| v.as_str())
        .unwrap_or("fallback")
        .to_string();
    let kind = args
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();
    let context_max_characters = args
        .get("contextMaxCharacters")
        .and_then(|v| v.as_f64())
        .map(|v| v as i64);

    let provider = select_web_search_provider(&ctx.session_id, exa, parallel);
    let title = web_search_provider_label(provider);
    ctx.metadata(crate::model::Metadata {
        title: Some(format!("{title} \"{query}\"")),
        metadata: serde_json::json!({ "provider": provider }),
    })?;

    ctx.ask(PermissionRequest {
        permission: "websearch".to_string(),
        patterns: vec![query.clone()],
        always: vec!["*".to_string()],
        metadata: serde_json::json!({
            "query": query,
            "numResults": num_results,
            "livecrawl": livecrawl,
            "type": kind,
            "contextMaxCharacters": context_max_characters,
            "provider": provider,
        }),
    })?;

    let result = call_provider(
        provider,
        &query,
        num_results,
        &livecrawl,
        &kind,
        context_max_characters,
        &ctx.extra,
        &ctx.session_id,
    )
    .await
    .map_err(|error| ToolError::Other(format!("Unable to search the web for {query}: {error}")))?;

    Ok(ExecuteResult {
        output: result.unwrap_or_else(|| {
            "No search results found. Please try a different query.".to_string()
        }),
        title: format!("{title}: {query}"),
        metadata: serde_json::json!({ "provider": provider }),
        attachments: None,
    })
}

async fn call_provider(
    provider: &str,
    query: &str,
    num_results: Option<i64>,
    livecrawl: &str,
    kind: &str,
    context_max_characters: Option<i64>,
    extra: &serde_json::Value,
    session_id: &str,
) -> Result<Option<String>, String> {
    let client = crate::http::client();
    if provider == "parallel" {
        let args = mcp_websearch::ParallelSearchArgs {
            objective: query.to_string(),
            search_queries: vec![query.to_string()],
            session_id: Some(session_id.to_string()),
            model_name: web_search_model_name(extra),
        };
        mcp_websearch::call(
            client,
            mcp_websearch::PARALLEL_URL,
            "web_search",
            serde_json::to_value(args).map_err(|error| error.to_string())?,
            parallel_auth_headers(),
        )
        .await
    } else {
        let args = mcp_websearch::SearchArgs {
            query: query.to_string(),
            kind: kind.to_string(),
            num_results: num_results.unwrap_or(8),
            livecrawl: livecrawl.to_string(),
            context_max_characters,
        };
        mcp_websearch::call(
            client,
            &mcp_websearch::exa_endpoint(),
            "web_search_exa",
            serde_json::to_value(args).map_err(|error| error.to_string())?,
            Vec::new(),
        )
        .await
    }
}

fn current_year() -> i32 {
    use chrono::Datelike;
    chrono::Local::now().year()
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
                    "contextMaxCharacters": {
                        "description": "Maximum characters for context string optimized for LLMs (default: 10000)",
                        "type": "number"
                    },
                    "livecrawl": {
                        "description": "Live crawl mode - 'fallback': use live crawling as backup if cached content unavailable, 'preferred': prioritize live crawling (default: 'fallback')",
                        "enum": ["fallback", "preferred"],
                        "type": "string"
                    },
                    "numResults": { "description": "Number of search results to return (default: 8)", "type": "number" },
                    "query": { "description": "Websearch query", "type": "string" },
                    "type": {
                        "description": "Search type - 'auto': balanced search (default), 'fast': quick results, 'deep': comprehensive search",
                        "enum": ["auto", "fast", "deep"],
                        "type": "string"
                    }
                },
                "required": ["query"],
                "type": "object"
            })
        );
    }

    #[test]
    fn description_embeds_year() {
        let description = prompts::WEBSEARCH.replace("{{year}}", &current_year().to_string());
        assert!(description.contains(&current_year().to_string()));
    }

    #[test]
    fn provider_selection_is_deterministic() {
        let a = select_web_search_provider("ses_abc", false, false);
        let b = select_web_search_provider("ses_abc", false, false);
        assert_eq!(a, b);
    }
}
