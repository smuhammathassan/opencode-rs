//! Golden tests: the opencode tool registry must expose the exact reference
//! tool set with JSON schemas matching the reference snapshots
//! (`reference/packages/opencode/test/tool/__snapshots__/parameters.test.ts.snap`).

use oc_tool::schema::{opt_prop, prop, Schema};
use oc_tool::tool::registry::{AgentInfo, ModelInfo, RuntimeFlags, ToolRegistry};

fn registry() -> ToolRegistry {
    ToolRegistry::new(
        RuntimeFlags {
            client: "cli",
            enable_question_tool: true,
            ..Default::default()
        },
        vec![AgentInfo {
            name: "general".into(),
            description: Some("General purpose agent".into()),
            mode: "primary".into(),
            permission: vec![],
        }],
    )
}

#[allow(dead_code)]
fn schema_of(schema: &Schema) -> serde_json::Value {
    oc_tool::jsonschema::from_schema(schema)
}

fn assert_schema(name: &str, expected: serde_json::Value) {
    let registry = registry();
    let tools = registry.all();
    let tool = tools
        .iter()
        .find(|tool| tool.id == name)
        .unwrap_or_else(|| panic!("missing tool {name}"));
    assert_eq!(
        tool.json_schema(),
        expected,
        "JSON Schema mismatch for tool {name}"
    );
}

#[test]
fn apply_patch_schema_matches_reference_snapshot() {
    assert_schema(
        "apply_patch",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "patchText": { "description": "The full patch text that describes all changes to be made", "type": "string" }
            },
            "required": ["patchText"],
            "type": "object"
        }),
    );
}

#[test]
fn bash_schema_matches_reference_snapshot() {
    assert_schema(
        "bash",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "command": { "description": "The command to execute", "type": "string" },
                "timeout": {
                    "description": "Optional timeout in milliseconds",
                    "exclusiveMinimum": 0,
                    "maximum": 9007199254740991i64,
                    "minimum": -9007199254740991i64,
                    "type": "integer"
                },
                "workdir": {
                    "description": "The working directory to run the command in. Defaults to the current directory. Use this instead of 'cd' commands.",
                    "type": "string"
                }
            },
            "required": ["command"],
            "type": "object"
        }),
    );
}

#[test]
fn edit_schema_matches_reference_snapshot() {
    assert_schema(
        "edit",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "filePath": { "description": "The absolute path to the file to modify", "type": "string" },
                "newString": { "description": "The text to replace it with (must be different from oldString)", "type": "string" },
                "oldString": { "description": "The text to replace", "type": "string" },
                "replaceAll": { "description": "Replace all occurrences of oldString (default false)", "type": "boolean" }
            },
            "required": ["filePath", "oldString", "newString"],
            "type": "object"
        }),
    );
}

#[test]
fn glob_grep_schemas_match_reference_snapshots() {
    assert_schema(
        "glob",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "path": {
                    "description": "The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the default behavior. Must be a valid directory path if provided.",
                    "type": "string"
                },
                "pattern": { "description": "The glob pattern to match files against", "type": "string" }
            },
            "required": ["pattern"],
            "type": "object"
        }),
    );
    assert_schema(
        "grep",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "include": { "description": "File pattern to include in the search (e.g. \"*.js\", \"*.{ts,tsx}\")", "type": "string" },
                "path": { "description": "The directory to search in. Defaults to the current working directory.", "type": "string" },
                "pattern": { "description": "The regex pattern to search for in file contents", "type": "string" }
            },
            "required": ["pattern"],
            "type": "object"
        }),
    );
}

#[test]
fn read_schema_matches_reference_snapshot() {
    assert_schema(
        "read",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "filePath": { "description": "The absolute path to the file or directory to read", "type": "string" },
                "limit": { "description": "The maximum number of lines to read (defaults to 2000)", "maximum": 9007199254740991i64, "minimum": 0, "type": "integer" },
                "offset": { "description": "The line number to start reading from (1-indexed)", "maximum": 9007199254740991i64, "minimum": 0, "type": "integer" }
            },
            "required": ["filePath"],
            "type": "object"
        }),
    );
}

#[test]
fn task_schema_matches_reference_snapshot() {
    // The LLM-facing `jsonSchema` uses `BaseParameters` (no `background`).
    assert_schema(
        "task",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "command": { "description": "The command that triggered this task", "type": "string" },
                "description": { "description": "A short (3-5 words) description of the task", "type": "string" },
                "prompt": { "description": "The task for the agent to perform", "type": "string" },
                "subagent_type": { "description": "The type of specialized agent to use for this task", "type": "string" },
                "task_id": {
                    "description": "This should only be set if you mean to resume a previous task (you can pass a prior task_id and the task will continue the same subagent session as before instead of creating a fresh one)",
                    "type": "string"
                }
            },
            "required": ["description", "prompt", "subagent_type"],
            "type": "object"
        }),
    );
}

#[test]
fn webfetch_websearch_schemas_match_reference_snapshots() {
    assert_schema(
        "webfetch",
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
        }),
    );
    assert_schema(
        "websearch",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "contextMaxCharacters": { "description": "Maximum characters for context string optimized for LLMs (default: 10000)", "type": "number" },
                "livecrawl": { "description": "Live crawl mode - 'fallback': use live crawling as backup if cached content unavailable, 'preferred': prioritize live crawling (default: 'fallback')", "enum": ["fallback", "preferred"], "type": "string" },
                "numResults": { "description": "Number of search results to return (default: 8)", "type": "number" },
                "query": { "description": "Websearch query", "type": "string" },
                "type": { "description": "Search type - 'auto': balanced search (default), 'fast': quick results, 'deep': comprehensive search", "enum": ["auto", "fast", "deep"], "type": "string" }
            },
            "required": ["query"],
            "type": "object"
        }),
    );
}

#[test]
fn question_todo_skill_invalid_schemas_match_reference_snapshots() {
    assert_schema(
        "question",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "questions": {
                    "description": "Questions to ask",
                    "items": {
                        "properties": {
                            "header": { "description": "Very short label (max 30 chars)", "type": "string" },
                            "multiple": { "description": "Allow selecting multiple choices", "type": "boolean" },
                            "options": {
                                "description": "Available choices",
                                "items": {
                                    "properties": {
                                        "description": { "description": "Explanation of choice", "type": "string" },
                                        "label": { "description": "Display text (1-5 words, concise)", "type": "string" }
                                    },
                                    "required": ["label", "description"],
                                    "type": "object"
                                },
                                "type": "array"
                            },
                            "question": { "description": "Complete question", "type": "string" }
                        },
                        "required": ["question", "header", "options"],
                        "type": "object"
                    },
                    "type": "array"
                }
            },
            "required": ["questions"],
            "type": "object"
        }),
    );
    assert_schema(
        "todowrite",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "todos": {
                    "description": "The updated todo list",
                    "items": {
                        "properties": {
                            "content": { "description": "Brief description of the task", "type": "string" },
                            "priority": { "description": "Priority level of the task: high, medium, low", "type": "string" },
                            "status": { "description": "Current status of the task: pending, in_progress, completed, cancelled", "type": "string" }
                        },
                        "required": ["content", "status", "priority"],
                        "type": "object"
                    },
                    "type": "array"
                }
            },
            "required": ["todos"],
            "type": "object"
        }),
    );
    assert_schema(
        "skill",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "name": { "description": "The name of the skill from available_skills", "type": "string" }
            },
            "required": ["name"],
            "type": "object"
        }),
    );
    assert_schema(
        "invalid",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "error": { "type": "string" },
                "tool": { "type": "string" }
            },
            "required": ["tool", "error"],
            "type": "object"
        }),
    );
}

#[test]
fn write_schema_matches_reference_snapshot() {
    assert_schema(
        "write",
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "content": { "description": "The content to write to the file", "type": "string" },
                "filePath": { "description": "The absolute path to the file to write (must be absolute, not relative)", "type": "string" }
            },
            "required": ["content", "filePath"],
            "type": "object"
        }),
    );
}

#[test]
fn registry_exposes_exact_reference_tool_set() {
    let registry = registry();
    let ids = registry.ids();
    let expected = [
        "invalid",
        "question",
        "bash",
        "read",
        "glob",
        "grep",
        "edit",
        "write",
        "task",
        "webfetch",
        "todowrite",
        "websearch",
        "skill",
        "apply_patch",
    ];
    assert_eq!(ids, expected);
}

#[test]
fn description_strings_match_reference_prompts() {
    let registry = registry();
    let all = registry.all();
    let by_id: std::collections::HashMap<&str, &oc_tool::tool::tool::Def> =
        all.iter().map(|def| (def.id.as_str(), def)).collect();
    // read.txt / write.txt / edit.txt are embedded verbatim.
    assert_eq!(by_id["read"].description, oc_tool::prompts::READ);
    assert_eq!(by_id["write"].description, oc_tool::prompts::WRITE);
    assert_eq!(by_id["edit"].description, oc_tool::prompts::EDIT);
    assert_eq!(by_id["glob"].description, oc_tool::prompts::GLOB);
    assert_eq!(by_id["grep"].description, oc_tool::prompts::GREP);
    assert_eq!(by_id["webfetch"].description, oc_tool::prompts::WEBFETCH);
    assert_eq!(by_id["skill"].description, oc_tool::prompts::SKILL);
    assert_eq!(by_id["todowrite"].description, oc_tool::prompts::TODOWRITE);
    assert_eq!(by_id["question"].description, oc_tool::prompts::QUESTION);
    assert_eq!(
        by_id["apply_patch"].description,
        oc_tool::prompts::APPLY_PATCH
    );
    assert_eq!(by_id["task"].description, oc_tool::prompts::TASK);
    assert!(by_id["websearch"]
        .description
        .starts_with("- Search the web using the session's web search provider"));
}

#[test]
fn model_filter_swaps_apply_patch_for_gpt_models() {
    let registry = registry();
    let agent = registry.agents[0].clone();
    let gpt = registry.tools(
        &ModelInfo {
            provider_id: "opencode".into(),
            model_id: "gpt-5".into(),
        },
        &agent,
    );
    assert!(gpt.iter().any(|tool| tool.id == "apply_patch"));
    assert!(gpt
        .iter()
        .all(|tool| tool.id != "edit" && tool.id != "write"));

    let other = registry.tools(
        &ModelInfo {
            provider_id: "anthropic".into(),
            model_id: "claude-sonnet".into(),
        },
        &agent,
    );
    assert!(other.iter().any(|tool| tool.id == "edit"));
    assert!(other.iter().all(|tool| tool.id != "apply_patch"));
}

#[test]
fn core_builtins_expose_reference_tool_names() {
    let registry = oc_tool::core::registry_with_builtins(false, false);
    let materialization = registry.materialize(&[]);
    let mut names: Vec<&str> = materialization
        .definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "apply_patch",
            "bash",
            "edit",
            "glob",
            "grep",
            "question",
            "read",
            "skill",
            "task",
            "todowrite",
            "webfetch",
            "websearch",
            "write"
        ]
    );
}

#[test]
fn opt_prop_and_prop_are_exported() {
    let _ = prop("a", Schema::plain_string());
    let _ = opt_prop("b", Schema::plain_string());
}
