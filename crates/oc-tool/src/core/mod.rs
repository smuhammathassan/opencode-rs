//! V2 core tool engine: registry + built-in leaves.
//!
//! Mirrors `reference/packages/core/src/tool/`.

pub mod bash;
pub mod edit;
pub mod glob_grep;
pub mod misc;
pub mod read;
pub mod read_filesystem;
pub mod registry;
pub mod tool;
pub mod tool_output_store;
pub mod write;

use registry::{ApplicationTools, CoreToolRegistry};

/// `BuiltInTools.node` composition from
/// `reference/packages/core/src/tool/builtins.ts` — the shipped Location-scoped
/// built-in set.
pub fn builtins(enable_exa: bool, enable_parallel: bool) -> Vec<(String, tool::CoreTool)> {
    vec![
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
    ]
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
                "todowrite",
                "webfetch",
                "websearch",
                "write"
            ]
        );
    }
}
