//! Slash command templates.
//!
//! From reference/packages/opencode/src/command/index.ts. `Info` mirrors the
//! `Command` schema there; `hints`/`render` port the template analysis and the
//! `$ARGUMENTS`/`$1..$9` substitution from
//! reference/packages/opencode/src/session/prompt.ts (lines 1372-1409).

use crate::frontmatter;
use crate::skill;
use crate::util::{scan, ScanOptions};
use indexmap::IndexMap;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::path::Path;
use std::sync::Arc;

pub const INIT: &str = "init";
pub const REVIEW: &str = "review";

const PROMPT_INITIALIZE: &str = include_str!("template/initialize.txt");
const PROMPT_REVIEW: &str = include_str!("template/review.txt");

/// `$1..$9` hints for a command template.
/// From reference/packages/opencode/src/command/index.ts (`hints`).
pub fn hints(template: &str) -> Vec<String> {
    let mut result: Vec<String> = placeholder_regex()
        .find_iter(template)
        .map(|m| m.as_str().to_string())
        .collect();
    result.sort();
    result.dedup();
    if template.contains("$ARGUMENTS") {
        result.push("$ARGUMENTS".to_string());
    }
    result
}

/// Render a command template with the user-provided arguments.
///
/// Mirrors `reference/packages/opencode/src/session/prompt.ts`:
/// numbered placeholders map to positional arguments (the highest-numbered
/// placeholder consumes the remaining arguments), `$ARGUMENTS` is replaced
/// verbatim, and when the template has no placeholders the arguments are
/// appended. Shell (`!`cmd``) expansion is performed separately with
/// [`expand_shell`]; callers then trim the result.
pub fn render(template: &str, arguments: &str) -> String {
    let args: Vec<String> = args_regex()
        .find_iter(arguments)
        .map(|m| trim_quotes(m.as_str()).to_string())
        .collect();
    let placeholders: Vec<String> = placeholder_regex()
        .find_iter(template)
        .map(|m| m.as_str().to_string())
        .collect();
    let last = placeholders
        .iter()
        .filter_map(|p| p[1..].parse::<usize>().ok())
        .max()
        .unwrap_or(0);

    let mut with_args = String::with_capacity(template.len());
    let mut last_end = 0;
    for captures in placeholder_regex().captures_iter(template) {
        let whole = captures.get(0).expect("whole match");
        with_args.push_str(&template[last_end..whole.start()]);
        let position: usize = captures[1].parse().unwrap_or(0);
        let arg_index = position.saturating_sub(1);
        if arg_index < args.len() {
            if position == last {
                with_args.push_str(&args[arg_index..].join(" "));
            } else {
                with_args.push_str(&args[arg_index]);
            }
        }
        last_end = whole.end();
    }
    with_args.push_str(&template[last_end..]);

    let uses_arguments = template.contains("$ARGUMENTS");
    let mut result = with_args.replace("$ARGUMENTS", arguments);
    if placeholders.is_empty() && !uses_arguments && !arguments.trim().is_empty() {
        result.push_str("\n\n");
        result.push_str(arguments);
    }
    result.trim().to_string()
}

/// Expand `!`cmd`` segments by running each command (nothrow semantics: a
/// failing command contributes an empty string).
/// From reference/packages/opencode/src/session/prompt.ts (lines 1397-1408)
/// and reference/packages/opencode/src/config/markdown.ts (`SHELL_REGEX`).
pub fn expand_shell(
    template: &str,
    run: &dyn Fn(&str) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let mut result = String::with_capacity(template.len());
    let mut last_end = 0;
    for captures in shell_regex().captures_iter(template) {
        let whole = captures.get(0).expect("whole match");
        result.push_str(&template[last_end..whole.start()]);
        let command = captures.get(1).expect("command group").as_str();
        result.push_str(&run(command).unwrap_or_default());
        last_end = whole.end();
    }
    result.push_str(&template[last_end..]);
    Ok(result)
}

/// Command source, mirroring the `source` literal union in `Command.Info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Command,
    Mcp,
    Skill,
}

/// Lazily-resolved command template. The reference holds either a string or a
/// promise (MCP prompt resolution); resolution happens on access.
/// From reference/packages/opencode/src/command/index.ts (`Info.template`).
#[derive(Clone)]
pub struct Template(Arc<dyn Fn() -> String + Send + Sync>);

impl Template {
    pub fn new<F>(resolve: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        Template(Arc::new(resolve))
    }

    pub fn static_value(value: impl Into<String>) -> Self {
        let value = value.into();
        Template(Arc::new(move || value.clone()))
    }

    pub fn resolve(&self) -> String {
        (self.0)()
    }
}

impl std::fmt::Debug for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Template").finish_non_exhaustive()
    }
}

/// Command info, mirroring `Command.Info` from
/// reference/packages/opencode/src/command/index.ts.
#[derive(Debug, Clone)]
pub struct Info {
    pub name: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub source: Option<Source>,
    pub template: Template,
    pub subtask: Option<bool>,
    pub hints: Vec<String>,
}

impl Info {
    pub fn from_config(name: &str, config: &CommandConfig) -> Self {
        Info {
            name: name.to_string(),
            description: config.description.clone(),
            agent: config.agent.clone(),
            model: config.model.clone(),
            source: Some(Source::Command),
            template: Template::static_value(config.template.clone()),
            subtask: config.subtask,
            hints: hints(&config.template),
        }
    }

    pub fn from_skill(skill: &skill::Info) -> Self {
        let template = skill_template(skill);
        Info {
            name: skill.name.clone(),
            description: skill.description.clone(),
            agent: None,
            model: None,
            source: Some(Source::Skill),
            template: Template::static_value(template),
            subtask: None,
            hints: Vec::new(),
        }
    }

    /// Render this command's template with arguments.
    pub fn render(&self, arguments: &str) -> String {
        render(&self.template.resolve(), arguments)
    }
}

/// Metadata needed to expose an MCP prompt as a slash command.
///
/// The reference command service fetches the prompt template lazily. Keeping
/// this metadata in the registry (rather than in `Info`) lets the server do
/// the same without making the synchronous command model depend on an async
/// MCP client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPrompt {
    /// The sanitized `client:prompt` command key shown to callers.
    pub command_name: String,
    /// The configured MCP client name used for `prompts/get`.
    pub client: String,
    /// The native prompt name sent to the MCP server.
    pub name: String,
    pub description: Option<String>,
    /// Argument names in the order advertised by `prompts/list`.
    pub arguments: Vec<String>,
}

impl McpPrompt {
    /// Build the placeholder arguments used to retrieve a prompt template.
    /// The resolved template is rendered with the user's actual slash-command
    /// arguments afterward, matching the reference's `$1`, `$2`, ... mapping.
    pub fn request_arguments(&self) -> serde_json::Value {
        let mut arguments = serde_json::Map::new();
        for (index, name) in self.arguments.iter().enumerate() {
            arguments.insert(
                name.clone(),
                serde_json::Value::String(format!("${}", index + 1)),
            );
        }
        serde_json::Value::Object(arguments)
    }
}

impl Serialize for Info {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Command", 8)?;
        state.serialize_field("name", &self.name)?;
        if let Some(description) = &self.description {
            state.serialize_field("description", description)?;
        }
        if let Some(agent) = &self.agent {
            state.serialize_field("agent", agent)?;
        }
        if let Some(model) = &self.model {
            state.serialize_field("model", model)?;
        }
        if let Some(source) = &self.source {
            state.serialize_field("source", source)?;
        }
        state.serialize_field("template", &self.template.resolve())?;
        if let Some(subtask) = &self.subtask {
            state.serialize_field("subtask", subtask)?;
        }
        state.serialize_field("hints", &self.hints)?;
        state.end()
    }
}

/// One entry of the `command` section in the merged config.
/// From reference/packages/core/src/v1/config/command.ts (`ConfigCommandV1.Info`).
#[derive(Debug, Clone, Deserialize)]
pub struct CommandConfig {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub subtask: Option<bool>,
}

/// Load project command files from a `.opencode` directory.
///
/// From reference/packages/opencode/src/config/command.ts (`ConfigCommand.load`):
/// scans `{command,commands}/**/*.md`, parses frontmatter, and derives the
/// command name from the file path relative to `dir`. Files with invalid
/// frontmatter are skipped; files with invalid config fields abort the load.
///
/// TODO(integration): oc-config owns this in the reference (`ConfigCommand.load`);
/// consider moving it there once oc-config is implemented.
pub fn load_from_dir(dir: &Path) -> anyhow::Result<Vec<(String, CommandConfig)>> {
    let mut result: Vec<(String, CommandConfig)> = Vec::new();
    for item in scan(
        dir,
        "{command,commands}/**/*.md",
        &ScanOptions {
            dot: true,
            follow: true,
        },
    )? {
        let Some(md) = frontmatter::parse_file(&item).ok() else {
            continue;
        };
        let relative = item.strip_prefix(dir).unwrap_or(&item);
        let name = config_entry_name_from_path(relative, &["command/", "commands/"]);
        let (name, config) = merge_command_config(&name, &md).map_err(|error| {
            anyhow::anyhow!("{}: invalid command config: {}", item.display(), error)
        })?;
        result.push((name, config));
    }
    Ok(result)
}

/// `{ name, ...md.data, template: md.content.trim() }` merged and validated.
fn merge_command_config(
    name: &str,
    md: &frontmatter::Markdown,
) -> Result<(String, CommandConfig), String> {
    let mut merged = if md.data.is_object() {
        md.data.clone()
    } else {
        serde_json::Value::Object(Default::default())
    };
    let mut final_name = name.to_string();
    if let Some(object) = merged.as_object_mut() {
        // frontmatter `name` overrides the path-derived name.
        if let Some(frontmatter_name) = object.get("name").and_then(serde_json::Value::as_str) {
            final_name = frontmatter_name.to_string();
        }
        object.insert(
            "template".to_string(),
            serde_json::Value::String(md.content.trim().to_string()),
        );
    }
    let config = serde_json::from_value(merged).map_err(|error| error.to_string())?;
    Ok((final_name, config))
}

/// Derive a command name from a path relative to the scanned directory.
/// From reference/packages/opencode/src/config/entry-name.ts
/// (`configEntryNameFromPath`).
fn config_entry_name_from_path(relative: &Path, prefixes: &[&str]) -> String {
    let normalized = relative.to_string_lossy().replace('\\', "/");
    let mut candidate: Option<String> = None;
    for prefix in prefixes {
        if normalized.starts_with(prefix) {
            candidate = Some(normalized[prefix.len()..].to_string());
            break;
        }
    }
    let candidate = match candidate {
        Some(candidate) => candidate,
        None => relative
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
    };
    match Path::new(&candidate).extension() {
        Some(extension) if !extension.is_empty() => {
            candidate[..candidate.len() - extension.to_string_lossy().len() - 1].to_string()
        }
        _ => candidate,
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid command configuration: {0}")]
pub struct ConfigCommandError(String);

/// Registry of slash commands, mirroring the `commands` state in
/// reference/packages/opencode/src/command/index.ts. Default `init`/`review`
/// commands are registered first, then config commands (which may override the
/// defaults), then skills (which never override existing commands).
#[derive(Debug, Default)]
pub struct Registry {
    commands: IndexMap<String, Info>,
    mcp_prompts: IndexMap<String, McpPrompt>,
}

impl Registry {
    pub fn new(worktree: impl AsRef<Path>) -> Self {
        let worktree = worktree.as_ref().display().to_string();
        let mut registry = Registry::default();

        registry.commands.insert(
            INIT.to_string(),
            Info {
                name: INIT.to_string(),
                description: Some("guided AGENTS.md setup".to_string()),
                agent: None,
                model: None,
                source: Some(Source::Command),
                template: Template::static_value(PROMPT_INITIALIZE.replace("${path}", &worktree)),
                subtask: None,
                hints: hints(PROMPT_INITIALIZE),
            },
        );
        registry.commands.insert(
            REVIEW.to_string(),
            Info {
                name: REVIEW.to_string(),
                description: Some(
                    "review changes [commit|branch|pr], defaults to uncommitted".to_string(),
                ),
                agent: None,
                model: None,
                source: Some(Source::Command),
                template: Template::static_value(PROMPT_REVIEW.replace("${path}", &worktree)),
                subtask: Some(true),
                hints: hints(PROMPT_REVIEW),
            },
        );
        registry
    }

    pub fn get(&self, name: &str) -> Option<&Info> {
        self.commands.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Info> {
        self.commands.get_mut(name)
    }

    pub fn list(&self) -> impl Iterator<Item = &Info> {
        self.commands.values()
    }

    /// Register commands from the merged config `command` section.
    pub fn add_config_commands(
        &mut self,
        commands: &serde_json::Value,
    ) -> Result<(), ConfigCommandError> {
        let object = commands
            .as_object()
            .ok_or_else(|| ConfigCommandError("`command` must be an object".to_string()))?;
        for (name, value) in object {
            let config: CommandConfig = serde_json::from_value(value.clone())
                .map_err(|error| ConfigCommandError(error.to_string()))?;
            self.commands
                .insert(name.clone(), Info::from_config(name, &config));
        }
        Ok(())
    }

    /// Register pre-validated command entries, e.g. from [`load_from_dir`].
    pub fn add_config_entries<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = (String, CommandConfig)>,
    {
        for (name, config) in entries {
            self.commands
                .insert(name.clone(), Info::from_config(&name, &config));
        }
    }

    /// Register skills as commands (source "skill").
    pub fn add_skills(&mut self, skills: &[skill::Info]) {
        for skill in skills {
            if self.commands.contains_key(&skill.name) {
                continue;
            }
            self.commands
                .insert(skill.name.clone(), Info::from_skill(skill));
        }
    }

    /// Register MCP prompts as lazy slash commands. The actual `prompts/get`
    /// call is performed by the server when the command is executed.
    ///
    /// Existing commands are intentionally replaced, matching the reference
    /// command service's MCP prompt loop; skills added afterward still do not
    /// override the prompt command.
    pub fn add_mcp_prompts<I>(&mut self, prompts: I)
    where
        I: IntoIterator<Item = McpPrompt>,
    {
        for prompt in prompts {
            let command_name = prompt.command_name.clone();
            let hints = prompt
                .arguments
                .iter()
                .enumerate()
                .map(|(index, _)| format!("${}", index + 1))
                .collect();
            self.commands.insert(
                command_name.clone(),
                Info {
                    name: command_name.clone(),
                    description: prompt.description.clone(),
                    agent: None,
                    model: None,
                    source: Some(Source::Mcp),
                    template: Template::static_value(String::new()),
                    subtask: None,
                    hints,
                },
            );
            self.mcp_prompts.insert(command_name, prompt);
        }
    }

    /// Return the backing MCP metadata for a registered prompt command.
    pub fn get_mcp_prompt(&self, name: &str) -> Option<&McpPrompt> {
        self.mcp_prompts.get(name)
    }
}

/// Skill command template: the skill body plus a base-directory note.
/// From reference/packages/opencode/src/command/index.ts (skill loop).
fn skill_template(skill: &skill::Info) -> String {
    if skill.location == "<built-in>" {
        return skill.content.clone();
    }
    let dir = Path::new(&skill.location)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!(
        "{}\n\nBase directory for this skill: {}\nRelative paths in this skill (e.g., scripts/, references/) are relative to this base directory.",
        skill.content, dir
    )
}

fn trim_quotes(arg: &str) -> &str {
    let arg = if arg.starts_with('"') || arg.starts_with('\'') {
        &arg[1..]
    } else {
        arg
    };
    if arg.ends_with('"') || arg.ends_with('\'') {
        &arg[..arg.len() - 1]
    } else {
        arg
    }
}

/// `/\\$(\\d+)/` from reference/packages/opencode/src/session/prompt.ts.
fn placeholder_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\$(\d+)").expect("valid placeholder regex"))
}

/// `argsRegex` from reference/packages/opencode/src/session/prompt.ts.
fn args_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)\[Image\s+\d+\]|"[^"]*"|'[^']*'|[^\s"']+"#)
            .expect("valid args regex")
    })
}

/// `SHELL_REGEX` from reference/packages/opencode/src/config/markdown.ts.
fn shell_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"!`([^`]+)`").expect("valid shell regex"))
}
