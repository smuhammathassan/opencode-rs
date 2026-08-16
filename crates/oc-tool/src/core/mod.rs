//! V2 core tool engine: registry + built-in leaves.
//!
//! Mirrors `reference/packages/core/src/tool/`.

pub mod bash;
pub mod edit;
pub mod glob_grep;
pub mod lsp;
pub mod misc;
pub mod plan;
pub mod read;
pub mod read_filesystem;
pub mod registry;
pub mod task;
pub mod tool;
pub mod tool_output_store;
pub mod write;

use registry::{ApplicationTools, CoreToolRegistry};

/// `BuiltInTools.node` composition from
/// `reference/packages/core/src/tool/builtins.ts` — the shipped Location-scoped
/// built-in set.
pub fn builtins(enable_exa: bool, enable_parallel: bool) -> Vec<(String, tool::CoreTool)> {
    builtins_with_options(enable_exa, enable_parallel, false)
}

/// Build the shipped tools with host-controlled experimental capabilities.
/// The server owns the feature flag because the task tool's background mode
/// requires a lifecycle-aware child-session host.
pub fn builtins_with_options(
    enable_exa: bool,
    enable_parallel: bool,
    enable_background_subagents: bool,
) -> Vec<(String, tool::CoreTool)> {
    builtins_with_lsp_options(
        enable_exa,
        enable_parallel,
        enable_background_subagents,
        false,
        false,
    )
}

pub fn builtins_with_lsp_options(
    enable_exa: bool,
    enable_parallel: bool,
    enable_background_subagents: bool,
    enable_plan_mode: bool,
    enable_lsp: bool,
) -> Vec<(String, tool::CoreTool)> {
    let mut tools = vec![
        ("read".to_string(), read::def()),
        ("write".to_string(), write::def()),
        ("edit".to_string(), edit::def()),
        ("bash".to_string(), bash::def()),
        ("glob".to_string(), glob_grep::glob::def()),
        ("grep".to_string(), glob_grep::grep::def()),
        ("webfetch".to_string(), misc::webfetch::def()),
        (
            "websearch".to_string(),
            misc::websearch::def(enable_exa, enable_parallel),
        ),
        ("todowrite".to_string(), misc::todowrite::def()),
        ("question".to_string(), misc::question::def()),
        ("skill".to_string(), misc::skill::def()),
        ("apply_patch".to_string(), misc::apply_patch::def()),
        ("task".to_string(), task::def(enable_background_subagents)),
    ];
    if enable_plan_mode {
        tools.push((plan::NAME.to_string(), plan::def()));
    }
    if enable_lsp {
        tools.push((lsp::NAME.to_string(), lsp::def()));
    }
    tools
}

/// Build the shipped tools with experimental plan-mode support.
pub fn builtins_with_plan_options(
    enable_exa: bool,
    enable_parallel: bool,
    enable_background_subagents: bool,
    enable_plan_mode: bool,
) -> Vec<(String, tool::CoreTool)> {
    builtins_with_lsp_options(
        enable_exa,
        enable_parallel,
        enable_background_subagents,
        enable_plan_mode,
        false,
    )
}

/// A registry pre-loaded with the shipped built-ins.
pub fn registry_with_builtins(enable_exa: bool, enable_parallel: bool) -> CoreToolRegistry {
    let mut registry = CoreToolRegistry::new(ApplicationTools::default());
    let _guard = registry
        .register(builtins(enable_exa, enable_parallel))
        .expect("built-in tool names are valid");
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_definitions_are_exposed() {
        let registry = registry_with_builtins(false, false);
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
    fn skill_builtin_loads_project_skill_markdown() {
        let root = std::env::temp_dir().join(format!("opencode-core-skill-{}", std::process::id()));
        let skill_dir = root.join(".opencode/skills/demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Demo skill\nUse it.\n").unwrap();

        let mut context = tool::CoreContext {
            session_id: "ses_skill".into(),
            agent: "build".into(),
            assistant_message_id: "msg_skill".into(),
            tool_call_id: "call_skill".into(),
            location_directory: root.to_string_lossy().into_owned(),
            asks: Vec::new(),
            subagent_depth: None,
            subagent_parent_depth: std::sync::Arc::new(|_| 0),
            execute_subagent: None,
            lsp_request: None,
        };
        let call = crate::model::ToolCall {
            id: "call_skill".into(),
            name: "skill".into(),
            input: serde_json::json!({"name": "demo"}),
        };
        let settled = tool::settle(&misc::skill::def(), &call, &mut context).unwrap();
        assert_eq!(settled.structured["name"], "demo");
        assert!(settled.structured["output"]
            .as_str()
            .unwrap()
            .contains("# Demo skill"));
        assert!(context.asks.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plan_exit_is_opt_in() {
        let without_plan = builtins_with_plan_options(false, false, false, false);
        assert!(!without_plan.iter().any(|(name, _)| name == plan::NAME));
        let with_plan = builtins_with_plan_options(false, false, false, true);
        assert!(with_plan.iter().any(|(name, _)| name == plan::NAME));
    }
}
