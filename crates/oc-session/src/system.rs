/// From reference/packages/opencode/src/session/system.ts
///
/// System prompt assembly. The provider-specific prompt templates are embedded
/// verbatim from the reference `prompt/*.txt` files (assets/prompt/*.txt).
use crate::permission;
use crate::provider::ProviderModel;
use crate::v1::Ruleset;

const PROMPT_ANTHROPIC: &str = include_str!("../assets/prompt/anthropic.txt");
const PROMPT_DEFAULT: &str = include_str!("../assets/prompt/default.txt");
const PROMPT_BEAST: &str = include_str!("../assets/prompt/beast.txt");
const PROMPT_GEMINI: &str = include_str!("../assets/prompt/gemini.txt");
const PROMPT_GPT: &str = include_str!("../assets/prompt/gpt.txt");
const PROMPT_KIMI: &str = include_str!("../assets/prompt/kimi.txt");
const PROMPT_META: &str = include_str!("../assets/prompt/meta.txt");
const PROMPT_CODEX: &str = include_str!("../assets/prompt/codex.txt");
const PROMPT_TRINITY: &str = include_str!("../assets/prompt/trinity.txt");

/// From reference `system.ts:provider` — selects the model-specific prompt.
pub fn provider(model: &ProviderModel) -> &'static str {
    let api_id = &model.api.id;
    if api_id.contains("muse-spark") {
        PROMPT_META
    } else if api_id.contains("gpt-4") || api_id.contains("o1") || api_id.contains("o3") {
        PROMPT_BEAST
    } else if api_id.contains("gpt") {
        if api_id.contains("codex") {
            PROMPT_CODEX
        } else {
            PROMPT_GPT
        }
    } else if api_id.contains("gemini-") {
        PROMPT_GEMINI
    } else if api_id.contains("claude") {
        PROMPT_ANTHROPIC
    } else if api_id.to_lowercase().contains("trinity") {
        PROMPT_TRINITY
    } else if api_id.to_lowercase().contains("kimi") {
        PROMPT_KIMI
    } else {
        PROMPT_DEFAULT
    }
}

#[derive(Debug, Clone)]
pub struct EnvironmentContext {
    pub directory: String,
    pub worktree: String,
    pub vcs_is_git: bool,
    pub platform: String,
    pub today: String,
}

#[derive(Debug, Clone)]
pub struct ProjectReference {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
}

/// From reference `system.ts:environment`. Returns the env block and an
/// optional references block.
pub fn environment(
    model: &ProviderModel,
    ctx: &EnvironmentContext,
    references: &[ProjectReference],
) -> Vec<String> {
    let env = [
        format!(
            "You are powered by the model named {}. The exact model ID is {}/{}",
            model.api.id, model.provider_id, model.api.id
        ),
        "Here is some useful information about the environment you are running in:".to_string(),
        "<env>".to_string(),
        format!("  Working directory: {}", ctx.directory),
        format!("  Workspace root folder: {}", ctx.worktree),
        format!(
            "  Is directory a git repo: {}",
            if ctx.vcs_is_git { "yes" } else { "no" }
        ),
        format!("  Platform: {}", ctx.platform),
        format!("  Today's date: {}", ctx.today),
        "</env>".to_string(),
    ]
    .join("\n");

    let mut result = vec![env];
    if !references.is_empty() {
        let mut sorted = references.to_vec();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        let mut lines = vec![
            "Project references provide additional directories that can be accessed when relevant."
                .to_string(),
            "<available_references>".to_string(),
        ];
        for reference in sorted {
            lines.push("  <reference>".to_string());
            lines.push(format!("    <name>{}</name>", reference.name));
            lines.push(format!("    <path>{}</path>", reference.path));
            if let Some(description) = &reference.description {
                lines.push(format!("    <description>{description}</description>"));
            }
            lines.push("  </reference>".to_string());
        }
        lines.push("</available_references>".to_string());
        result.push(lines.join("\n"));
    }
    result
}

#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: Option<String>,
    pub location: String,
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// From reference `packages/opencode/src/skill/index.ts:fmt`.
pub fn fmt_skills(list: &[SkillInfo], verbose: bool) -> String {
    let described: Vec<&SkillInfo> = list
        .iter()
        .filter(|skill| skill.description.is_some())
        .collect();
    if described.is_empty() {
        return "No skills are currently available.".to_string();
    }
    let mut sorted = described;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    if verbose {
        let mut lines = vec!["<available_skills>".to_string()];
        for skill in sorted {
            lines.push("  <skill>".to_string());
            lines.push(format!("    <name>{}</name>", skill.name));
            lines.push(format!(
                "    <description>{}</description>",
                skill.description.clone().unwrap_or_default()
            ));
            lines.push(format!(
                "    <location>{}</location>",
                escape_html(&skill.location)
            ));
            lines.push("  </skill>".to_string());
        }
        lines.push("</available_skills>".to_string());
        return lines.join("\n");
    }
    let mut lines = vec!["## Available Skills".to_string()];
    for skill in sorted {
        lines.push(format!(
            "- **{}**: {}",
            skill.name,
            skill.description.clone().unwrap_or_default()
        ));
    }
    lines.join("\n")
}

/// From reference `system.ts:skills`.
pub fn skills(agent_permission: &Ruleset, available: &[SkillInfo]) -> Option<String> {
    let disabled_names = permission::disabled(&["skill".to_string()], agent_permission);
    if disabled_names.contains("skill") {
        return None;
    }
    Some(
        [
            "Skills provide specialized instructions and workflows for specific tasks.".to_string(),
            "Use the skill tool to load a skill when a task matches its description.".to_string(),
            fmt_skills(available, true),
        ]
        .join("\n"),
    )
}

#[derive(Debug, Clone)]
pub struct McpInstruction {
    pub name: String,
    pub tools: Vec<String>,
    pub instructions: String,
}

/// From reference `system.ts:mcp`.
pub fn mcp(
    agent_permission: &Ruleset,
    permission: Option<&Ruleset>,
    instructions: &[McpInstruction],
) -> Option<String> {
    let ruleset = permission::merge([agent_permission, permission.unwrap_or(&vec![])]);
    let filtered: Vec<&McpInstruction> = instructions
        .iter()
        .filter(|item| {
            item.tools.is_empty()
                || permission::disabled(&item.tools, &ruleset).len() < item.tools.len()
        })
        .collect();
    if filtered.is_empty() {
        return None;
    }
    let mut lines = vec!["<mcp_instructions>".to_string()];
    for item in filtered {
        lines.push(format!("  <server name=\"{}\">", item.name));
        for line in item.instructions.split('\n') {
            lines.push(format!("    {line}"));
        }
        lines.push("  </server>".to_string());
    }
    lines.push("</mcp_instructions>".to_string());
    Some(lines.join("\n"))
}

/// From reference `packages/opencode/src/session/prompt.ts` — assembly of the
/// system prompt passed to the model for a turn.
pub fn assemble(
    env: &[String],
    instructions: &[String],
    mcp_instructions: Option<&str>,
    skills_block: Option<&str>,
) -> Vec<String> {
    let mut system: Vec<String> = Vec::new();
    system.extend(env.iter().cloned());
    system.extend(instructions.iter().cloned());
    if let Some(mcp_instructions) = mcp_instructions {
        system.push(mcp_instructions.to_string());
    }
    if let Some(skills_block) = skills_block {
        system.push(skills_block.to_string());
    }
    system
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderApiInfo, ProviderLimit, ProviderModel};

    fn model(api_id: &str) -> ProviderModel {
        ProviderModel {
            id: api_id.to_string(),
            provider_id: "openai".to_string(),
            api: ProviderApiInfo {
                id: api_id.to_string(),
                npm: None,
                type_: "native".to_string(),
            },
            name: api_id.to_string(),
            family: None,
            capabilities: Default::default(),
            cost: Default::default(),
            limit: ProviderLimit {
                context: 0.0,
                input: None,
                output: 0.0,
            },
            status: "active".to_string(),
            options: Default::default(),
            headers: Default::default(),
            release_date: String::new(),
            variants: None,
        }
    }

    #[test]
    fn provider_selects_gpt_beast_prompt() {
        let prompt = provider(&model("gpt-4o"));
        assert!(prompt.starts_with("You are opencode, an agent - please keep going until the user"));
    }

    #[test]
    fn provider_defaults_for_unknown() {
        let prompt = provider(&model("my-model"));
        assert!(prompt.starts_with("You are opencode, an interactive CLI tool"));
    }

    #[test]
    fn provider_anthropic() {
        let prompt = provider(&model("claude-sonnet-4"));
        assert!(prompt.starts_with("You are OpenCode, the best coding agent on the planet."));
    }

    #[test]
    fn environment_block_matches_reference() {
        let model = model("gpt-4o-mini");
        let ctx = EnvironmentContext {
            directory: "/home/user/project".into(),
            worktree: "/home/user/project".into(),
            vcs_is_git: true,
            platform: "linux".into(),
            today: "Mon Aug 05 2026".into(),
        };
        let blocks = environment(&model, &ctx, &[]);
        assert_eq!(blocks.len(), 1);
        let expected = "You are powered by the model named gpt-4o-mini. The exact model ID is openai/gpt-4o-mini\nHere is some useful information about the environment you are running in:\n<env>\n  Working directory: /home/user/project\n  Workspace root folder: /home/user/project\n  Is directory a git repo: yes\n  Platform: linux\n  Today's date: Mon Aug 05 2026\n</env>";
        assert_eq!(blocks[0], expected);
    }

    #[test]
    fn environment_sorts_references() {
        let model = model("gpt-4o");
        let ctx = EnvironmentContext {
            directory: "/p".into(),
            worktree: "/p".into(),
            vcs_is_git: false,
            platform: "linux".into(),
            today: "Mon Aug 05 2026".into(),
        };
        let references = vec![
            ProjectReference {
                name: "zeta".into(),
                path: "/r/z".into(),
                description: Some("Z".into()),
            },
            ProjectReference {
                name: "alpha".into(),
                path: "/r/a".into(),
                description: None,
            },
        ];
        let blocks = environment(&model, &ctx, &references);
        assert_eq!(blocks.len(), 2);
        let idx_alpha = blocks[1].find("<name>alpha</name>").unwrap();
        let idx_zeta = blocks[1].find("<name>zeta</name>").unwrap();
        assert!(idx_alpha < idx_zeta);
        assert!(blocks[1].contains("<description>Z</description>"));
    }
}
