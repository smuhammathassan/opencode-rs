//! V2 core leaves without dedicated modules: webfetch, websearch, todowrite,
//! question, skill, apply_patch.
//!
//! Ports of `reference/packages/core/src/tool/{webfetch,websearch,todowrite,question,skill,apply-patch}.ts`.

use crate::core::tool::{self, CoreContext, CoreTool};
use crate::model::{Content, ToolError};
use crate::schema::{opt_prop, prop, Schema};

pub mod webfetch {
    use super::*;
    use crate::tool::webfetch::{convert_html_to_markdown, extract_text_from_html};

    pub const NAME: &str = "webfetch";
    pub const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
    pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
    pub const MAX_TIMEOUT_SECONDS: u64 = 120;

    /// `Input` from `reference/packages/core/src/tool/webfetch.ts:27`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![
                prop(
                    "url",
                    Schema::string("The HTTP or HTTPS URL to fetch content from"),
                ),
                opt_prop(
                    "format",
                    Schema::literals(
                        &["text", "markdown", "html"],
                        "The format to return the content in. Defaults to markdown.",
                    )
                    .with_default(serde_json::json!("markdown")),
                ),
                opt_prop(
                    "timeout",
                    Schema::number().with_description(format!(
                        "Optional timeout in seconds (maximum: {MAX_TIMEOUT_SECONDS})"
                    )),
                ),
            ],
            "webfetch",
        )
    }

    /// `WebFetchTool` from `reference/packages/core/src/tool/webfetch.ts:118`.
    pub fn def() -> CoreTool {
        tool::make(
            "Fetch content from an HTTP or HTTPS URL and return it as text, markdown, or HTML. Markdown is the default.\n\nUse a more targeted tool when one is available. This tool is read-only. Large text results may be replaced with a preview while the complete output is retained in managed storage.",
            input(),
            output_schema(),
            None,
            None,
            Some(std::sync::Arc::new(|_input, output| {
                let text = output.get("output").and_then(|v| v.as_str()).unwrap_or("").to_string();
                vec![Content::Text { text }]
            })),
            execute,
        )
    }

    fn output_schema() -> Schema {
        Schema::struct_(
            vec![
                prop("url", Schema::plain_string()),
                prop("contentType", Schema::plain_string()),
                prop(
                    "format",
                    Schema::literals(&["text", "markdown", "html"], "format"),
                ),
                prop("output", Schema::plain_string()),
            ],
            "webfetch",
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
    ) -> Result<serde_json::Value, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let format = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("markdown")
            .to_string();
        let timeout = input.get("timeout").and_then(|v| v.as_f64());

        let parsed = url::Url::parse(&url)
            .map_err(|_| ToolError::failure(format!("Unable to fetch {url}")))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(ToolError::failure(format!("Unable to fetch {url}")));
        }

        context.assert(crate::core::tool::CorePermissionRequest {
            action: NAME.to_string(),
            resources: vec![url.clone()],
            save: Some(vec!["*".to_string()]),
            metadata: Some(input.clone()),
            source: source(context),
        })?;

        let accept = accept_header(&format);
        let client = crate::http::client();
        let response = crate::core::tool::run_future(Box::pin(fetch(
            &client,
            &url,
            &accept,
            timeout
                .unwrap_or(DEFAULT_TIMEOUT_SECONDS as f64)
                .min(MAX_TIMEOUT_SECONDS as f64),
        )))?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = crate::core::tool::run_future(Box::pin(async {
            response
                .bytes()
                .await
                .map_err(|error| ToolError::Other(error.to_string()))
        }))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(ToolError::failure(format!(
                "Response too large (exceeds {MAX_RESPONSE_BYTES} byte limit)"
            )));
        }
        let content = String::from_utf8_lossy(&bytes).to_string();
        let output = if content_type.contains("text/html") {
            match format.as_str() {
                "markdown" => convert_html_to_markdown(&content),
                "text" => extract_text_from_html(&content),
                _ => content,
            }
        } else {
            content
        };

        Ok(serde_json::json!({
            "url": url,
            "contentType": content_type,
            "format": format,
            "output": output,
        }))
    }

    async fn fetch(
        client: &reqwest::Client,
        url: &str,
        accept: &str,
        timeout_seconds: f64,
    ) -> Result<reqwest::Response, ToolError> {
        let headers = [
            ("User-Agent".to_string(), BROWSER_UA.to_string()),
            ("Accept".to_string(), accept.to_string()),
            ("Accept-Language".to_string(), "en-US,en;q=0.9".to_string()),
        ];
        tokio::time::timeout(
            std::time::Duration::from_secs_f64(timeout_seconds),
            send(client, url, &headers),
        )
        .await
        .map_err(|_| ToolError::failure("Request timed out"))?
    }

    async fn send(
        client: &reqwest::Client,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<reqwest::Response, ToolError> {
        let mut request = client.get(url);
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ToolError::Other(format!(
                "HTTP status {}",
                response.status()
            )));
        }
        Ok(response)
    }

    const BROWSER_UA: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

    fn accept_header(format: &str) -> &'static str {
        match format {
            "markdown" => "text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1",
            "text" => "text/plain;q=1.0, text/markdown;q=0.9, text/html;q=0.8, */*;q=0.1",
            "html" => {
                "text/html;q=1.0, application/xhtml+xml;q=0.9, text/plain;q=0.8, text/markdown;q=0.7, */*;q=0.1"
            }
            _ => "*/*",
        }
    }

    fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
        crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        }
    }
}

pub mod websearch {
    use super::*;

    pub const NAME: &str = "websearch";
    pub const NO_RESULTS: &str = "No search results found. Please try a different query.";
    pub const EXA_URL: &str = "https://mcp.exa.ai/mcp";
    pub const PARALLEL_URL: &str = "https://search.parallel.ai/mcp";
    pub const MAX_NUM_RESULTS: i64 = 20;
    pub const MAX_CONTEXT_CHARACTERS: i64 = 50_000;

    /// `Input` from `reference/packages/core/src/tool/websearch.ts:40`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![
                prop("query", Schema::string("Websearch query")),
                opt_prop(
                    "numResults",
                    Schema::positive_int().with_description(format!(
                        "Number of search results to return (default: 8, maximum: {MAX_NUM_RESULTS})"
                    )),
                ),
                opt_prop(
                    "livecrawl",
                    Schema::literals(
                        &["fallback", "preferred"],
                        "Live crawl mode - 'fallback': use live crawling as backup if cached unavailable, 'preferred': prioritize live crawling (default: 'fallback')",
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
                    Schema::positive_int().with_description(format!(
                        "Maximum characters for context string optimized for models (default: 10000, maximum: {MAX_CONTEXT_CHARACTERS})"
                    )),
                ),
            ],
            "websearch",
        )
    }

    /// `selectProvider` from `reference/packages/core/src/tool/websearch.ts:88`.
    pub fn select_provider(
        session_id: &str,
        enable_exa: bool,
        enable_parallel: bool,
    ) -> &'static str {
        let override_provider = std::env::var("OPENCODE_WEBSEARCH_PROVIDER").ok();
        match override_provider.as_deref() {
            Some("exa") => return "exa",
            Some("parallel") => return "parallel",
            _ => {}
        }
        if enable_parallel {
            return "parallel";
        }
        if enable_exa {
            return "exa";
        }
        match crate::checksum::checksum(session_id)
            .and_then(|value| u64::from_str_radix(&value, 36).ok())
        {
            Some(parsed) if parsed % 2 == 0 => "exa",
            _ => "parallel",
        }
    }

    /// `WebSearchTool` from `reference/packages/core/src/tool/websearch.ts:192`.
    pub fn def(enable_exa: bool, enable_parallel: bool) -> CoreTool {
        let description = format!(
            "Search the web using the session's local web search provider. Use this for current information beyond knowledge cutoff.\n\nThis is a provider-independent local tool backed by Exa or Parallel. Provider-hosted web search tools are separate and execute at the model provider.\n\nOptional controls support result count, live crawling ('fallback' or 'preferred'), search type ('auto', 'fast', or 'deep'), and maximum context characters.\n\nThe current year is {}. Use this year when searching for recent information or current events.",
            crate::util::current_year()
        );
        tool::make(
            description,
            input(),
            output_schema(),
            None,
            None,
            Some(std::sync::Arc::new(|_input, output| {
                let text = output
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                vec![Content::Text { text }]
            })),
            move |input, context| execute(input, context, enable_exa, enable_parallel),
        )
    }

    fn output_schema() -> Schema {
        Schema::struct_(
            vec![
                prop(
                    "provider",
                    Schema::literals(&["exa", "parallel"], "provider"),
                ),
                prop("text", Schema::plain_string()),
            ],
            "websearch",
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
        enable_exa: bool,
        enable_parallel: bool,
    ) -> Result<serde_json::Value, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let provider = select_provider(&context.session_id, enable_exa, enable_parallel);

        context.assert(crate::core::tool::CorePermissionRequest {
            action: NAME.to_string(),
            resources: vec![query.clone()],
            save: Some(vec!["*".to_string()]),
            metadata: Some(serde_json::json!({ "provider": provider })),
            source: source(context),
        })?;

        let text = crate::core::tool::run_future(Box::pin(call_provider(
            provider,
            &query,
            input.clone(),
            &context.session_id,
        )))?;
        Ok(serde_json::json!({
            "provider": provider,
            "text": text.unwrap_or_else(|| NO_RESULTS.to_string()),
        }))
    }

    async fn call_provider(
        provider: &str,
        query: &str,
        input: serde_json::Value,
        session_id: &str,
    ) -> Result<Option<String>, ToolError> {
        let client = crate::http::client();
        if provider == "parallel" {
            let args = serde_json::json!({
                "objective": query,
                "search_queries": [query],
                "session_id": session_id,
            });
            crate::tool::mcp_websearch::call(
                &client,
                PARALLEL_URL,
                "web_search",
                args,
                vec![("User-Agent".to_string(), "opencode/local".to_string())],
            )
            .await
            .map_err(ToolError::Other)
        } else {
            let args = serde_json::json!({
                "query": query,
                "type": input.get("type").and_then(|v| v.as_str()).unwrap_or("auto"),
                "numResults": input.get("numResults").and_then(|v| v.as_i64()).unwrap_or(8),
                "livecrawl": input.get("livecrawl").and_then(|v| v.as_str()).unwrap_or("fallback"),
                "contextMaxCharacters": input.get("contextMaxCharacters").and_then(|v| v.as_i64()),
            });
            crate::tool::mcp_websearch::call(&client, EXA_URL, "web_search_exa", args, Vec::new())
                .await
                .map_err(ToolError::Other)
        }
    }

    fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
        crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        }
    }
}

pub mod todowrite {
    use super::*;

    pub const NAME: &str = "todowrite";

    /// `Input` from `reference/packages/core/src/tool/todowrite.ts:14`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![prop(
                "todos",
                Schema::array(todo_info_schema(), "The updated todo list"),
            )],
            "todowrite",
        )
    }

    /// `toModelOutput` from `reference/packages/core/src/tool/todowrite.ts:23`.
    pub fn to_model_output(output: &serde_json::Value) -> String {
        serde_json::to_string_pretty(output.get("todos").unwrap_or(&serde_json::Value::Null))
            .unwrap_or_else(|_| String::new())
    }

    /// `TodoWriteTool` from `reference/packages/core/src/tool/todowrite.ts:25`.
    pub fn def() -> CoreTool {
        tool::make(
            "Create and maintain a structured task list for the current coding session. Use it to track progress during multi-step work and keep todo statuses current.",
            input(),
            output_schema(),
            None,
            None,
            Some(std::sync::Arc::new(|_input, output| {
                vec![Content::Text {
                    text: to_model_output(output),
                }]
            })),
            execute,
        )
    }

    fn output_schema() -> Schema {
        Schema::struct_(
            vec![prop(
                "todos",
                Schema::array(todo_info_schema(), "The updated todo list"),
            )],
            "todowrite",
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
    ) -> Result<serde_json::Value, ToolError> {
        let todos = input.get("todos").cloned().unwrap_or(serde_json::json!([]));
        context.assert(crate::core::tool::CorePermissionRequest {
            action: NAME.to_string(),
            resources: vec!["*".to_string()],
            save: Some(vec!["*".to_string()]),
            metadata: None,
            source: source(context),
        })?;
        // TODO(integration): `SessionTodo.update`.
        Ok(serde_json::json!({ "todos": todos }))
    }

    fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
        crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        }
    }
}

pub mod question {
    use super::*;

    pub const NAME: &str = "question";

    /// `Input` from `reference/packages/core/src/tool/question.ts:25`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![prop(
                "questions",
                Schema::array(crate::tool::question::prompt_schema(), "Questions to ask"),
            )],
            "question",
        )
    }

    /// `QuestionTool` from `reference/packages/core/src/tool/question.ts:47`.
    pub fn def() -> CoreTool {
        tool::make(
            crate::tool::question::DESCRIPTION,
            input(),
            output_schema(),
            None,
            None,
            Some(std::sync::Arc::new(|input, output| {
                let questions = input
                    .get("questions")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));
                let answers = output
                    .get("answers")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));
                vec![Content::Text {
                    text: crate::tool::question::to_model_output(&questions, &answers),
                }]
            })),
            execute,
        )
    }

    fn output_schema() -> Schema {
        Schema::struct_(
            vec![prop(
                "answers",
                Schema::array(Schema::array(Schema::plain_string(), "answer"), "answers"),
            )],
            "question",
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
    ) -> Result<serde_json::Value, ToolError> {
        let questions = input
            .get("questions")
            .cloned()
            .unwrap_or(serde_json::json!([]));
        context.assert(crate::core::tool::CorePermissionRequest {
            action: "question".to_string(),
            resources: vec!["*".to_string()],
            save: None,
            metadata: None,
            source: source(context),
        })?;
        // TODO(integration): `QuestionV2.ask`.
        let _ = &questions;
        let answers: Vec<serde_json::Value> = Vec::new();
        Ok(serde_json::json!({ "answers": answers }))
    }

    fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
        crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        }
    }
}

pub mod skill {
    use super::*;

    pub const NAME: &str = "skill";
    pub const FILE_LIMIT: usize = 10;

    /// `Input` from `reference/packages/core/src/tool/skill.ts:17`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![prop(
                "name",
                Schema::string("The name of the skill from the available skills list"),
            )],
            "skill",
        )
    }

    /// `SkillTool` from `reference/packages/core/src/tool/skill.ts:57`.
    pub fn def() -> CoreTool {
        tool::make(
            crate::core::misc::skill::DESCRIPTION,
            input(),
            output_schema(),
            None,
            None,
            Some(std::sync::Arc::new(|_input, output| {
                let text = output
                    .get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                vec![Content::Text { text }]
            })),
            execute,
        )
    }

    const DESCRIPTION: &str = "Load a specialized skill when the task at hand matches one of the available skills in the system context.\n\nUse this tool to inject the skill's instructions and resources into the current conversation. The output may contain detailed workflow guidance as well as references to scripts, files, etc. in the same directory as the skill.\n\nThe skill name must match one of the available skills in the system context.";

    fn output_schema() -> Schema {
        Schema::struct_(
            vec![
                prop("name", Schema::plain_string()),
                prop("directory", Schema::plain_string()),
                prop("output", Schema::plain_string()),
            ],
            "skill",
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
    ) -> Result<serde_json::Value, ToolError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.trim().is_empty()
            || std::path::Path::new(&name)
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(ToolError::failure(format!("Unable to load skill {name}")));
        }

        let mut roots = vec![
            std::path::PathBuf::from(&context.location_directory).join(".opencode/skills"),
            std::path::PathBuf::from(&context.location_directory).join(".opencode/skill"),
            std::path::PathBuf::from(&context.location_directory).join("skills"),
        ];
        if let Some(directory) = std::env::var_os("OPENCODE_SKILL_DIR") {
            roots.insert(0, std::path::PathBuf::from(directory));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = std::path::PathBuf::from(home);
            roots.push(home.join(".config/opencode/skills"));
            roots.push(home.join(".agents/skills"));
        }

        for root in roots {
            let directory = root.join(&name);
            for filename in ["SKILL.md", "skill.md"] {
                let path = directory.join(filename);
                let Ok(output) = std::fs::read_to_string(&path) else {
                    continue;
                };
                return Ok(serde_json::json!({
                    "name": name,
                    "directory": directory,
                    "output": output,
                }));
            }
        }

        Err(ToolError::failure(format!("Unable to load skill {name}")))
    }
}

pub mod apply_patch {
    use super::*;
    use crate::patch::{self, Hunk};

    pub const NAME: &str = "apply_patch";

    /// `Input` from `reference/packages/core/src/tool/apply-patch.ts:19`.
    pub fn input() -> Schema {
        Schema::struct_(
            vec![prop(
                "patchText",
                Schema::string("The full patch text describing add, update, and delete operations"),
            )],
            "apply_patch",
        )
    }

    /// `toModelOutput` from `reference/packages/core/src/tool/apply-patch.ts:37`.
    pub fn to_model_output(output: &serde_json::Value) -> String {
        let mut lines = vec!["Applied patch sequentially:".to_string()];
        if let Some(applied) = output.get("applied").and_then(|v| v.as_array()) {
            for item in applied {
                let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let resource = item.get("resource").and_then(|v| v.as_str()).unwrap_or("");
                let mark = match kind {
                    "add" => "A",
                    "delete" => "D",
                    _ => "M",
                };
                lines.push(format!("{mark} {resource}"));
            }
        }
        lines.join("\n")
    }

    /// `ApplyPatchTool` from `reference/packages/core/src/tool/apply-patch.ts:59`.
    pub fn def() -> CoreTool {
        let tool = tool::make(
            "Apply one patch containing add, update, and delete file operations. All targets are resolved and approved before target contents are read. Operations apply sequentially; if a later operation fails, earlier operations remain applied and the failure reports them explicitly. Moves and atomic rollback are not supported yet.",
            input(),
            output_schema(),
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

    fn output_schema() -> Schema {
        Schema::struct_(
            vec![
                prop("applied", Schema::array(applied_schema(), "applied")),
                prop(
                    "files",
                    Schema::array(crate::core::edit::file_diff_schema(), "files"),
                ),
            ],
            "apply_patch",
        )
    }

    fn applied_schema() -> Schema {
        Schema::struct_(
            vec![
                prop(
                    "type",
                    Schema::literals(&["add", "update", "delete"], "type"),
                ),
                prop("resource", Schema::plain_string()),
                prop("target", Schema::plain_string()),
            ],
            "applied",
        )
    }

    fn execute(
        input: serde_json::Value,
        context: &mut CoreContext,
    ) -> Result<serde_json::Value, ToolError> {
        let patch_text = input
            .get("patchText")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if patch_text.trim().is_empty() {
            return Err(ToolError::failure("patchText is required"));
        }
        let hunks = patch::parse_patch(&patch_text).map_err(|error| {
            ToolError::failure(format!("apply_patch verification failed: {error}"))
        })?;
        if hunks.is_empty() {
            return Err(ToolError::failure("patch rejected: empty patch"));
        }
        let move_hunk = hunks.iter().any(|hunk| {
            matches!(
                hunk,
                Hunk::Update {
                    move_path: Some(_),
                    ..
                }
            )
        });
        if move_hunk {
            return Err(ToolError::failure(
                "apply_patch moves are not supported yet",
            ));
        }

        for hunk in &hunks {
            let target = crate::util::path_resolve(&context.location_directory, hunk.path());
            if !crate::util::fs_contains(&context.location_directory, &target) {
                context.assert(crate::core::tool::CorePermissionRequest {
                    action: "external_directory".to_string(),
                    resources: vec![format!("{}/*", parent_dir(&target))],
                    save: None,
                    metadata: Some(serde_json::json!({ "filepath": target })),
                    source: source(context),
                })?;
            }
        }
        let resources: Vec<String> = hunks
            .iter()
            .map(|hunk| {
                crate::util::path_relative(
                    &context.location_directory,
                    &crate::util::path_resolve(&context.location_directory, hunk.path()),
                )
            })
            .collect();
        context.assert(crate::core::tool::CorePermissionRequest {
            action: "edit".to_string(),
            resources,
            save: Some(vec!["*".to_string()]),
            metadata: None,
            source: source(context),
        })?;

        let mut applied = Vec::new();
        let mut files = Vec::new();
        for hunk in &hunks {
            let target = crate::util::path_resolve(&context.location_directory, hunk.path());
            let resource = crate::util::path_relative(&context.location_directory, &target);
            match hunk {
                Hunk::Add { contents, .. } => {
                    let content = if contents.is_empty() || contents.ends_with('\n') {
                        contents.clone()
                    } else {
                        format!("{contents}\n")
                    };
                    crate::tool::write::write_with_dirs(&target, &content).map_err(|_| {
                        ToolError::failure(format!("Unable to apply patch at {}", hunk.path()))
                    })?;
                    applied.push(serde_json::json!({ "type": "add", "resource": resource, "target": target }));
                    files.push(serde_json::json!({
                        "file": resource,
                        "patch": crate::diff::create_two_files_patch(&resource, &resource, "", &content),
                        "status": "added",
                        "additions": crate::diff::count_lines(&content),
                        "deletions": 0,
                    }));
                }
                Hunk::Delete { .. } => {
                    std::fs::remove_file(&target).map_err(|_| {
                        ToolError::failure(format!("Unable to apply patch at {}", hunk.path()))
                    })?;
                    applied.push(serde_json::json!({ "type": "delete", "resource": resource, "target": target }));
                }
                Hunk::Update { chunks, .. } => {
                    let original = std::fs::read_to_string(&target).map_err(|_| {
                        ToolError::failure(format!("Unable to apply patch at {}", hunk.path()))
                    })?;
                    let update = patch::derive_new_contents_from_chunks(&target, chunks, &original)
                        .map_err(|error| {
                            ToolError::failure(format!(
                                "Unable to apply patch at {}: {error}",
                                hunk.path()
                            ))
                        })?;
                    crate::tool::write::write_with_dirs(&target, &update.content).map_err(
                        |_| ToolError::failure(format!("Unable to apply patch at {}", hunk.path())),
                    )?;
                    applied.push(serde_json::json!({ "type": "update", "resource": resource, "target": target }));
                    files.push(serde_json::json!({
                        "file": resource,
                        "patch": crate::diff::create_two_files_patch(&resource, &resource, &original, &update.content),
                        "status": "modified",
                        "additions": 0,
                        "deletions": 0,
                    }));
                }
            }
        }

        Ok(serde_json::json!({ "applied": applied, "files": files }))
    }

    fn parent_dir(path: &str) -> String {
        std::path::Path::new(path)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string())
    }

    fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
        crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        }
    }
}

fn todo_info_schema() -> Schema {
    crate::tool::todo::todo_item_schema()
}
