//! Port of `reference/packages/opencode/src/tool/registry.ts`.
//!
//! Assembles the built-in tool set and applies the model/provider/agent
//! filtering the session runner uses before emitting tool definitions to the
//! LLM. Plugin tool discovery (`fromPlugin`) is deferred:
//! TODO(integration): wire `oc-plugin`'s registered `tool` definitions and the
//! `tool.definition` hook into `tools()`.

use crate::util::Rule;

use super::tool::Def;

/// `RuntimeFlags.Service`-shaped flags used by the registry.
#[derive(Debug, Clone)]
pub struct RuntimeFlags {
    pub client: &'static str,
    pub enable_question_tool: bool,
    pub experimental_lsp_tool: bool,
    pub experimental_plan_mode: bool,
    pub experimental_code_mode: bool,
    pub experimental_background_subagents: bool,
    pub enable_exa: bool,
    pub enable_parallel: bool,
    pub bash_default_timeout_ms: Option<u64>,
}

impl Default for RuntimeFlags {
    fn default() -> Self {
        RuntimeFlags {
            client: "cli",
            enable_question_tool: false,
            experimental_lsp_tool: false,
            experimental_plan_mode: false,
            experimental_code_mode: false,
            experimental_background_subagents: false,
            enable_exa: false,
            enable_parallel: false,
            bash_default_timeout_ms: None,
        }
    }
}

/// An `Agent.Info`-shaped value for registry filtering.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub description: Option<String>,
    pub mode: String,
    pub permission: Vec<Rule>,
}

/// Model resolution input for `tools()`.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub provider_id: String,
    pub model_id: String,
}

/// `webSearchEnabled` from `reference/packages/opencode/src/tool/registry.ts:58`.
pub fn web_search_enabled(provider_id: &str, exa: bool, parallel: bool) -> bool {
    provider_id == "opencode" || exa || parallel
}

pub struct ToolRegistry {
    pub flags: RuntimeFlags,
    builtin: Vec<Def>,
    custom: Vec<Def>,
    task: Def,
    read: Def,
    /// Agents available for subagent dispatch (`describeTask`).
    pub agents: Vec<AgentInfo>,
}

impl ToolRegistry {
    /// Assembles the built-in tool set in the reference registration order.
    pub fn new(flags: RuntimeFlags, agents: Vec<AgentInfo>) -> Self {
        let invalid = super::invalid::def();
        let question = super::question::def();
        let shell = super::shell::def(flags.bash_default_timeout_ms);
        let read = super::read::def();
        let glob = super::glob::def();
        let grep = super::grep::def();
        let edit = super::edit::def();
        let write = super::write::def();
        let task = super::task::def(flags.experimental_background_subagents);
        let webfetch = super::webfetch::def();
        let todo = super::todo::def();
        let websearch = super::websearch::def(flags.enable_exa, flags.enable_parallel);
        let skill = super::skill::def();
        let patch = super::apply_patch::def();
        let lsp = super::lsp::def();
        let plan = super::plan::def();

        let question_enabled =
            ["app", "cli", "desktop"].contains(&flags.client) || flags.enable_question_tool;

        let mut builtin: Vec<Def> = Vec::new();
        builtin.push(invalid);
        if question_enabled {
            builtin.push(question);
        }
        builtin.push(shell);
        builtin.push(read.clone());
        builtin.push(glob);
        builtin.push(grep);
        builtin.push(edit);
        builtin.push(write);
        builtin.push(task.clone());
        builtin.push(webfetch);
        builtin.push(todo);
        builtin.push(websearch);
        builtin.push(skill);
        builtin.push(patch);
        if flags.experimental_code_mode {
            builtin.push(super::code_mode::def());
        }
        if flags.experimental_lsp_tool {
            builtin.push(lsp);
        }
        if flags.experimental_plan_mode && flags.client == "cli" {
            builtin.push(plan);
        }

        ToolRegistry {
            flags,
            builtin,
            custom: Vec::new(),
            task,
            read,
            agents,
        }
    }

    /// `ToolRegistry.all` from `reference/packages/opencode/src/tool/registry.ts:251`.
    pub fn all(&self) -> Vec<Def> {
        let mut all = self.builtin.clone();
        all.extend(self.custom.clone());
        all
    }

    /// `ToolRegistry.ids` from `reference/packages/opencode/src/tool/registry.ts:256`.
    pub fn ids(&self) -> Vec<String> {
        self.all().into_iter().map(|tool| tool.id).collect()
    }

    /// `ToolRegistry.named` from `reference/packages/opencode/src/tool/registry.ts:337`.
    pub fn named(&self) -> (Def, Def) {
        (self.task.clone(), self.read.clone())
    }

    /// `ToolRegistry.tools` from `reference/packages/opencode/src/tool/registry.ts:286`.
    pub fn tools(&self, model: &ModelInfo, agent: &AgentInfo) -> Vec<Def> {
        let use_patch = model.model_id.contains("gpt-")
            && !model.model_id.contains("oss")
            && !model.model_id.contains("gpt-4");

        let filtered: Vec<Def> = self
            .all()
            .into_iter()
            .filter(|tool| match tool.id.as_str() {
                "websearch" => web_search_enabled(
                    &model.provider_id,
                    self.flags.enable_exa,
                    self.flags.enable_parallel,
                ),
                "apply_patch" => use_patch,
                "edit" | "write" => !use_patch,
                _ => true,
            })
            .collect();

        let code_mode_description = filtered.iter().any(|tool| tool.id == "execute")
            && self.agents.iter().any(|item| item.mode != "primary");

        filtered
            .into_iter()
            .filter_map(|tool| {
                if tool.id == "execute" && !code_mode_description {
                    return None;
                }
                let mut description = tool.description.clone();
                if tool.id == "task" {
                    description = format!("{}\n{}", description, self.describe_task(agent));
                }
                if tool.id == "execute" && code_mode_description {
                    // TODO(integration): describe the connected MCP catalog.
                    description = format!(
                        "{}\n{}",
                        description,
                        "Connected MCP tools are available to the confined interpreter."
                    );
                }
                Some(tool.with_description(description))
            })
            .collect()
    }

    /// `ToolRegistry.describeTask` from `reference/packages/opencode/src/tool/registry.ts:260`.
    pub fn describe_task(&self, agent: &AgentInfo) -> String {
        let mut items: Vec<&AgentInfo> = self
            .agents
            .iter()
            .filter(|item| item.mode != "primary")
            .collect();
        items.retain(|item| {
            crate::util::evaluate("task", &item.name, &[&agent.permission]).action != "deny"
        });
        items.sort_by(|a, b| a.name.cmp(&b.name));
        let description = items
            .iter()
            .map(|item| {
                format!(
                    "- {}: {}",
                    item.name,
                    item.description.clone().unwrap_or_else(|| {
                        "This subagent should only be called manually by the user.".to_string()
                    })
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("Available agent types and the tools they have access to:\n{description}")
    }

    /// Register a custom (plugin) tool definition.
    /// TODO(integration): plugin `ToolDefinition` conversion (`fromPlugin`).
    pub fn register_custom(&mut self, tool: Def) {
        self.custom.push(tool);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ToolRegistry {
        let flags = RuntimeFlags {
            client: "cli",
            enable_question_tool: true,
            ..Default::default()
        };
        let agents = vec![
            AgentInfo {
                name: "general".into(),
                description: Some("General purpose agent".into()),
                mode: "primary".into(),
                permission: vec![Rule {
                    permission: "task".into(),
                    pattern: "*".into(),
                    action: "allow".into(),
                }],
            },
            AgentInfo {
                name: "explore".into(),
                description: None,
                mode: "subagent".into(),
                permission: vec![],
            },
        ];
        ToolRegistry::new(flags, agents)
    }

    #[test]
    fn exposes_builtin_tool_ids_in_order() {
        let ids = registry().ids();
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
    fn model_filter_swaps_edit_for_apply_patch() {
        let registry = registry();
        let model = ModelInfo {
            provider_id: "opencode".into(),
            model_id: "gpt-5".into(),
        };
        let agent = registry.agents[0].clone();
        let tools = registry.tools(&model, &agent);
        let ids: Vec<&str> = tools.iter().map(|tool| tool.id.as_str()).collect();
        assert!(ids.contains(&"apply_patch"));
        assert!(!ids.contains(&"edit"));
        assert!(!ids.contains(&"write"));

        let model = ModelInfo {
            provider_id: "opencode".into(),
            model_id: "sonnet".into(),
        };
        let tools = registry.tools(&model, &agent);
        let ids: Vec<&str> = tools.iter().map(|tool| tool.id.as_str()).collect();
        assert!(ids.contains(&"edit"));
        assert!(ids.contains(&"write"));
        assert!(!ids.contains(&"apply_patch"));
    }

    #[test]
    fn task_description_lists_agents() {
        let registry = registry();
        let agent = registry.agents[0].clone();
        let description = registry.describe_task(&agent);
        assert!(description.starts_with("Available agent types and the tools they have access to:"));
        assert!(description
            .contains("- explore: This subagent should only be called manually by the user."));
    }
}
