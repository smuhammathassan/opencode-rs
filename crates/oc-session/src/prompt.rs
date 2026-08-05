/// From reference/packages/opencode/src/session/prompt.ts
///
/// User prompt assembly: input schemas, markdown reference resolution, the
/// user message builder, structured-output tool and command argument parsing.
use serde::{Deserialize, Serialize};

use crate::provider::ProviderModel;
use crate::v1::{FilePartSource, Info, Part, PartBase, PartTime, User, UserModel};
use crate::JsonMap;

pub const STRUCTURED_OUTPUT_DESCRIPTION: &str =
    "Use this tool to return your final response in the requested structured format.

IMPORTANT:
- You MUST call this tool exactly once at the end of your response
- The input must be valid JSON matching the required schema
- Complete all necessary research and tool calls BEFORE calling this tool
- This tool provides your final answer - no further actions are taken after calling it";

pub const STRUCTURED_OUTPUT_SYSTEM_PROMPT: &str =
    "IMPORTANT: The user has requested structured output. You MUST use the StructuredOutput tool to provide your final response. Do NOT respond with plain text - you MUST call the StructuredOutput tool with your answer formatted according to the schema.";

pub const MAX_MCP_RESOURCE_BLOB_BYTES: usize = 10 * 1024 * 1024;

pub const SUPPORTED_MCP_RESOURCE_ATTACHMENT_MIMES: [&str; 5] = [
    "application/pdf",
    "image/gif",
    "image/jpeg",
    "image/png",
    "image/webp",
];

pub fn mcp_resource_base64_size(value: &str) -> usize {
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

pub fn format_mcp_resource_bytes(value: usize) -> String {
    if value < 1024 {
        format!("{value} B")
    } else if value < 1024 * 1024 {
        format!("{} KB", (value as f64 / 1024.0).ceil())
    } else {
        format!("{} MB", (value as f64 / (1024.0 * 1024.0)).ceil())
    }
}

/// From reference `prompt.ts:isOrphanedInterruptedTool`.
pub fn is_orphaned_interrupted_tool(part: &crate::v1::ToolPart) -> bool {
    matches!(&part.state, crate::v1::ToolState::Error(state)
        if state.metadata.as_ref().is_some_and(|m| m.get("interrupted").and_then(|v| v.as_bool()).unwrap_or(false)))
}

/// `PromptInput.parts` union — reference `prompt.ts:PromptInput`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PromptPart {
    #[serde(rename = "text")]
    Text(TextPartInput),
    #[serde(rename = "file")]
    File(FilePartInput),
    #[serde(rename = "agent")]
    Agent(AgentPartInput),
    #[serde(rename = "subtask")]
    Subtask(SubtaskPartInput),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPartInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<PartTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePartInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<FilePartSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPartInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::v1::AgentPartSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtaskPartInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub prompt: String,
    pub description: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<crate::v1::ModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRefInput {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptInput {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRefInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_reply: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<JsonMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<crate::v1::OutputFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub parts: Vec<PromptPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellInput {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRefInput>,
    pub command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub arguments: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<FilePartInput>>,
}

#[derive(Debug, Clone)]
pub struct FileStat {
    pub is_dir: bool,
}

/// Agent info subset used by prompt resolution — mirrors `Agent.Info`.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub mode: String,
    pub model: Option<ModelRefInput>,
    pub variant: Option<String>,
    pub permission: crate::v1::Ruleset,
    pub steps: Option<u64>,
    pub hidden: bool,
}

/// File reference matcher — `ConfigMarkdown.FILE_REGEX` from
/// reference/packages/opencode/src/config/markdown.ts. Rust's `regex` crate
/// lacks lookbehind, so the negative lookbehind is checked manually.
pub fn files(template: &str) -> Vec<(String, String)> {
    lazy_name_regex(template)
}

fn lazy_name_regex(template: &str) -> Vec<(String, String)> {
    let name_re =
        regex::Regex::new(r"@(\.?[^\s`,.]*(?:\.[^\s`,.]+)*)").expect("file name regex is valid");
    let mut result = Vec::new();
    for capture in name_re.captures_iter(template) {
        let whole = capture.get(0).expect("whole match");
        let name = capture.get(1).expect("name group");
        let at_byte = whole.start();
        // Negative lookbehind `(?<![\w`])`
        let prev = template[..at_byte].chars().next_back();
        let blocked = prev.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '`');
        if blocked {
            continue;
        }
        result.push((whole.as_str().to_string(), name.as_str().to_string()));
    }
    result
}

/// From reference `prompt.ts:resolvePromptParts` — turns a template with
/// `@file`/`@agent` references into prompt parts.
pub fn resolve_prompt_parts(
    template: &str,
    worktree: &str,
    home: &str,
    deps: &dyn ResolveDeps,
) -> Vec<PromptPart> {
    let mut parts: Vec<PromptPart> = vec![PromptPart::Text(TextPartInput {
        id: None,
        text: template.to_string(),
        synthetic: None,
        ignored: None,
        time: None,
        metadata: None,
    })];
    let file_matches = files(template);
    let mut seen = std::collections::HashSet::new();
    for (_, name) in file_matches {
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        seen.insert(name.clone());
        let filepath = if let Some(rest) = name.strip_prefix("~/") {
            std::path::Path::new(home).join(rest)
        } else {
            std::path::Path::new(worktree).join(&name)
        };
        match deps.stat(&filepath.to_string_lossy()) {
            Ok(None) => {
                if let Some(agent) = deps.get_agent(&name).unwrap_or(None) {
                    parts.push(PromptPart::Agent(AgentPartInput {
                        id: None,
                        name: agent.name,
                        source: None,
                    }));
                }
            }
            Ok(Some(stat)) => {
                parts.push(PromptPart::File(FilePartInput {
                    id: None,
                    url: url::Url::from_file_path(&filepath)
                        .map(|url| url.to_string())
                        .unwrap_or_else(|_| format!("file://{}", filepath.to_string_lossy())),
                    filename: Some(name.clone()),
                    mime: if stat.is_dir {
                        "application/x-directory".to_string()
                    } else {
                        "text/plain".to_string()
                    },
                    source: None,
                }));
            }
            Err(_) => {}
        }
    }
    parts
}

pub trait ResolveDeps {
    fn stat(&self, path: &str) -> Result<Option<FileStat>, String>;
    fn get_agent(&self, name: &str) -> Result<Option<AgentInfo>, String>;
}

/// From reference `prompt.ts:createStructuredOutputTool` — strips `$schema`
/// and exposes the schema description text.
pub fn structured_output_schema(
    input: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut schema = input.clone();
    schema.remove("$schema");
    schema
}

/// Command argument parsing — reference `prompt.ts:command`.
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub args: Vec<String>,
    pub last_arg_index: usize,
    pub uses_arguments_placeholder: bool,
    pub template: String,
    pub bash_matches: Vec<String>,
}

/// From reference `prompt.ts` regexes.
pub const ARGS_REGEX: &str = r#"(?:\[Image\s+\d+\]|"[^"]*"|'[^']*'|[^\s"']+)"#;
pub const PLACEHOLDER_REGEX: &str = r"\$(\d+)";
pub const QUOTE_TRIM_REGEX: &str = r#"^["']|["']$"#;
pub const BASH_REGEX: &str = r"!`([^`]+)`";

/// Parses raw command arguments into tokens (quotes, `[Image N]`, bare words).
pub fn parse_args(arguments: &str) -> Vec<String> {
    let re = regex::Regex::new(ARGS_REGEX).expect("args regex is valid");
    let quote_trim = regex::Regex::new(QUOTE_TRIM_REGEX).expect("quote regex is valid");
    re.find_iter(arguments)
        .map(|m| quote_trim.replace(m.as_str(), "").to_string())
        .collect()
}

/// Applies `$1..$N` placeholders to a command template (reference `command`).
pub fn apply_placeholders(template: &str, args: &[String], arguments: &str) -> String {
    let placeholder = regex::Regex::new(PLACEHOLDER_REGEX).expect("placeholder regex is valid");
    let mut last = 0usize;
    for capture in placeholder.captures_iter(template) {
        let value: usize = capture
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        if value > last {
            last = value;
        }
    }
    let with_args = placeholder.replace_all(template, |captures: &regex::Captures| {
        let index: usize = captures
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let arg_index = index.saturating_sub(1);
        if arg_index >= args.len() {
            return String::new();
        }
        if index == last {
            return args[arg_index..].join(" ");
        }
        args[arg_index].clone()
    });
    let mut result = with_args.replace("$ARGUMENTS", arguments);
    let placeholder_count = placeholder.find_iter(template).count();
    if placeholder_count == 0 && !template.contains("$ARGUMENTS") && !arguments.trim().is_empty() {
        result = format!("{result}\n\n{arguments}");
    }
    result
}

/// Extract shell `!`cmd`` matches for substitution (reference `command`).
pub fn shell_matches(template: &str) -> Vec<String> {
    let re = regex::Regex::new(BASH_REGEX).expect("bash regex is valid");
    re.captures_iter(template)
        .filter_map(|capture| capture.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

/// Substitute shell command outputs back into the template (reference `command`).
pub fn substitute_shell(template: &str, outputs: &[String]) -> String {
    let re = regex::Regex::new(BASH_REGEX).expect("bash regex is valid");
    let mut result = template.to_string();
    for (index, capture) in re.captures_iter(template).enumerate() {
        let whole = capture.get(0).expect("whole");
        let output = outputs.get(index).cloned().unwrap_or_default();
        result = result.replacen(whole.as_str(), &output, 1);
    }
    result
}

/// Reference `prompt.ts:ensureTitle` — the title-generation prompt.
pub fn title_generation_messages(history: &[crate::v1::WithParts]) -> Vec<serde_json::Value> {
    history
        .iter()
        .filter(|m| !m.parts.is_empty())
        .map(|m| {
            let text = m
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::json!({ "role": m.info.role(), "content": text })
        })
        .collect()
}

/// From reference `prompt.ts` constants used by the subtask handler.
pub const SUBTASK_SUMMARY_TEXT: &str =
    "Summarize the task tool output above and continue with your task.";
pub const SHELL_USER_TEXT: &str = "The following tool was executed by the user";

/// Build a draft user message from a `PromptInput` — the core of
/// `createUserMessage`. Pure message construction; the `create_user_message`
/// orchestration (agent/model resolution, part resolution, persistence) is
/// provided by [`PromptService`].
pub fn build_user_info(input: &PromptInput, agent: &str, model: &UserModel) -> User {
    User {
        id: input
            .message_id
            .clone()
            .unwrap_or_else(|| crate::schema::create_message(None)),
        session_id: input.session_id.clone(),
        role: "user".to_string(),
        time: crate::v1::UserTime {
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        },
        tools: input.tools.clone(),
        agent: agent.to_string(),
        model: model.clone(),
        system: input.system.clone(),
        format: input.format.clone(),
        summary: None,
    }
}

/// Persistence/plugin seam for [`create_user_message`].
///
/// TODO(integration): implement against oc-session-runner's Session service
/// once the store lands.
pub trait PromptService {
    fn get_session(&self, session_id: &str) -> Result<Info, String>;
    fn set_agent_model(
        &self,
        session_id: &str,
        agent: &str,
        model: &crate::session::SessionModelRef,
        time: u64,
    ) -> Result<(), String>;
    fn update_message(&self, info: &Info) -> Result<(), String>;
    fn update_part(&self, part: &Part) -> Result<(), String>;
    fn trigger_chat_message(
        &self,
        session_id: &str,
        message: &User,
        parts: &[Part],
    ) -> Result<(), String>;
}

/// `createUserMessage` from reference `prompt.ts` — assigns part ids and
/// persists the message + parts through the provided service.
pub fn create_user_message(
    input: &PromptInput,
    info: &User,
    service: &dyn PromptService,
) -> Result<(User, Vec<Part>), String> {
    let current = service.get_session(&input.session_id)?;
    let (current_agent, current_model) = match &current {
        Info::User(user) => (user.agent.as_str(), Some(&user.model)),
        Info::Assistant(assistant) => (assistant.agent.as_str(), None),
    };
    let _ = current_agent;
    let agent_changed = match current_model {
        Some(current_model) => {
            current_model.provider_id != info.model.provider_id
                || current_model.model_id != info.model.model_id
                || current_model.variant.as_deref().unwrap_or("default")
                    != info.model.variant.as_deref().unwrap_or("default")
        }
        None => true,
    };
    if agent_changed {
        service.set_agent_model(
            &input.session_id,
            &info.agent,
            &crate::session::SessionModelRef {
                id: info.model.model_id.clone(),
                provider_id: info.model.provider_id.clone(),
                variant: info.model.variant.clone().or(Some("default".to_string())),
            },
            info.time.created,
        )?;
    }

    let mut parts: Vec<Part> = Vec::new();
    for part in &input.parts {
        let resolved = resolve_input_part(part, &input.session_id, info.id.as_str())?;
        parts.extend(resolved);
    }

    service.trigger_chat_message(&input.session_id, info, &parts)?;
    service.update_message(&Info::User(Box::new(info.clone())))?;
    for part in &parts {
        service.update_part(part)?;
    }
    Ok((info.clone(), parts))
}

fn resolve_input_part(
    part: &PromptPart,
    session_id: &str,
    message_id: &str,
) -> Result<Vec<Part>, String> {
    let assign = |base: PartBase| base;
    let _ = assign;
    let part_id = |id: Option<&str>| {
        id.map(|v| v.to_string())
            .unwrap_or_else(|| crate::schema::create_part(None))
    };
    Ok(match part {
        PromptPart::Text(text) => vec![Part::Text(crate::v1::TextPart {
            base: PartBase {
                id: part_id(text.id.as_deref()),
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
            },
            type_: "text".into(),
            text: text.text.clone(),
            synthetic: text.synthetic,
            ignored: text.ignored,
            time: text.time.clone(),
            metadata: text.metadata.clone(),
        })],
        PromptPart::File(file) => vec![Part::File(crate::v1::FilePart {
            base: PartBase {
                id: part_id(file.id.as_deref()),
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
            },
            type_: "file".into(),
            mime: file.mime.clone(),
            filename: file.filename.clone(),
            url: file.url.clone(),
            source: file.source.clone(),
        })],
        PromptPart::Agent(agent) => vec![Part::Agent(crate::v1::AgentPart {
            base: PartBase {
                id: part_id(agent.id.as_deref()),
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
            },
            type_: "agent".into(),
            name: agent.name.clone(),
            source: agent.source.clone(),
        })],
        PromptPart::Subtask(subtask) => vec![Part::Subtask(crate::v1::SubtaskPart {
            base: PartBase {
                id: part_id(subtask.id.as_deref()),
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
            },
            type_: "subtask".into(),
            prompt: subtask.prompt.clone(),
            description: subtask.description.clone(),
            agent: subtask.agent.clone(),
            model: subtask.model.clone(),
            command: subtask.command.clone(),
        })],
    })
}

/// Return the provider prompt for a model — convenience re-export.
pub fn provider_prompt(model: &ProviderModel) -> &'static str {
    crate::system::provider(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionModelRef;

    #[test]
    fn mcp_resource_size_calculation() {
        // "aGVsbG8=" -> 5 bytes ("hello")
        assert_eq!(mcp_resource_base64_size("aGVsbG8="), 5);
        assert_eq!(format_mcp_resource_bytes(500), "500 B");
        assert_eq!(format_mcp_resource_bytes(2048), "2 KB");
    }

    struct FakeDeps;

    impl ResolveDeps for FakeDeps {
        fn stat(&self, path: &str) -> Result<Option<FileStat>, String> {
            if path.ends_with("notes.md") {
                Ok(Some(FileStat { is_dir: false }))
            } else if path.ends_with("src") {
                Ok(Some(FileStat { is_dir: true }))
            } else {
                Ok(None)
            }
        }
        fn get_agent(&self, name: &str) -> Result<Option<AgentInfo>, String> {
            if name == "helper" {
                Ok(Some(AgentInfo {
                    name: "helper".into(),
                    mode: "subagent".into(),
                    model: None,
                    variant: None,
                    permission: vec![],
                    steps: None,
                    hidden: false,
                }))
            } else {
                Ok(None)
            }
        }
    }

    #[test]
    fn resolve_prompt_parts_resolves_files_agents_and_text() {
        let parts = resolve_prompt_parts(
            "look at @notes.md and @src and @helper",
            "/work",
            "/home",
            &FakeDeps,
        );
        assert_eq!(parts.len(), 4);
        assert!(matches!(&parts[1], PromptPart::File(f) if f.mime == "text/plain"));
        assert!(matches!(&parts[2], PromptPart::File(f) if f.mime == "application/x-directory"));
        assert!(matches!(&parts[3], PromptPart::Agent(a) if a.name == "helper"));
    }

    #[test]
    fn file_regex_ignores_email_like_references() {
        let matches = files("contact me at user@example.com and @notes.md");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].1, "notes.md");
    }

    #[test]
    fn is_orphaned_interrupted_tool_detection() {
        let mut metadata = JsonMap::new();
        metadata.insert("interrupted".into(), serde_json::Value::Bool(true));
        let part = crate::v1::ToolPart {
            base: PartBase {
                id: "p".into(),
                session_id: "s".into(),
                message_id: "m".into(),
            },
            type_: "tool".into(),
            call_id: "c".into(),
            tool: "bash".into(),
            state: crate::v1::ToolState::Error(crate::v1::ToolStateError {
                status: "error".into(),
                input: Default::default(),
                error: "Tool execution aborted".into(),
                metadata: Some(metadata),
                time: crate::v1::CompletedTime {
                    start: 0,
                    end: 1,
                    compacted: None,
                },
            }),
            metadata: None,
        };
        assert!(is_orphaned_interrupted_tool(&part));
    }

    #[test]
    fn placeholder_substitution_matches_reference() {
        let args = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let template = "run $1 and $2 with $3";
        let result = apply_placeholders(template, &args, "");
        assert_eq!(result, "run alpha and beta with gamma");
    }

    #[test]
    fn last_placeholder_joins_remaining_args() {
        let args = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = apply_placeholders("cmd $1 $2", &args, "");
        assert_eq!(result, "cmd a b c");
    }

    #[test]
    fn arguments_appended_when_no_placeholders() {
        let result = apply_placeholders("do the thing", &[], "extra args");
        assert_eq!(result, "do the thing\n\nextra args");
    }

    #[test]
    fn create_user_message_persists_and_switches_agent() {
        let model = UserModel {
            provider_id: "openai".into(),
            model_id: "gpt-4o".into(),
            variant: None,
        };
        let info = build_user_info(
            &PromptInput {
                session_id: "ses1".into(),
                parts: vec![PromptPart::Text(TextPartInput {
                    id: None,
                    text: "hi".into(),
                    synthetic: None,
                    ignored: None,
                    time: None,
                    metadata: None,
                })],
                ..Default::default()
            },
            "primary",
            &model,
        );
        struct Svc;
        impl PromptService for Svc {
            fn get_session(&self, _: &str) -> Result<Info, String> {
                Ok(Info::User(Box::new(User {
                    id: "msg_old".into(),
                    session_id: "ses1".into(),
                    role: "user".into(),
                    time: crate::v1::UserTime { created: 0 },
                    format: None,
                    summary: None,
                    agent: "build".into(),
                    model: UserModel {
                        provider_id: "anthropic".into(),
                        model_id: "claude".into(),
                        variant: None,
                    },
                    system: None,
                    tools: None,
                })))
            }
            fn set_agent_model(
                &self,
                _: &str,
                _: &str,
                _: &SessionModelRef,
                _: u64,
            ) -> Result<(), String> {
                Ok(())
            }
            fn update_message(&self, _: &Info) -> Result<(), String> {
                Ok(())
            }
            fn update_part(&self, _: &Part) -> Result<(), String> {
                Ok(())
            }
            fn trigger_chat_message(&self, _: &str, _: &User, _: &[Part]) -> Result<(), String> {
                Ok(())
            }
        }
        let (_, parts) = create_user_message(
            &PromptInput {
                session_id: "ses1".into(),
                parts: vec![PromptPart::Text(TextPartInput {
                    id: None,
                    text: "hi".into(),
                    synthetic: None,
                    ignored: None,
                    time: None,
                    metadata: None,
                })],
                ..Default::default()
            },
            &info,
            &Svc,
        )
        .unwrap();
        assert_eq!(parts.len(), 1);
        assert!(parts[0].id().starts_with("prt"));
    }
}
