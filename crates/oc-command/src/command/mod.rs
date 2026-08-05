//! Slash command templates.
//!
//! From reference/packages/opencode/src/command/index.ts. `Info` mirrors the
//! `Command` schema there; `hints`/`render` port the template analysis and the
//! `$ARGUMENTS`/`$1..$9` substitution from
//! reference/packages/opencode/src/session/prompt.ts (lines 1372-1409).

use crate::skill;
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
/// appended. Shell (`!`cmd``) expansion is not performed here.
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

    // TODO(integration): register MCP prompt commands (source "mcp") once
    // oc-mcp exposes `prompts()`/`getPrompt`; mirror
    // reference/packages/opencode/src/command/index.ts lines 105-132.
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
