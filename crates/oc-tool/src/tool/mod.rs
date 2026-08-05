//! Port of `reference/packages/opencode/src/tool/`.

pub mod external_directory;
pub mod glob;
pub mod grep;
pub mod invalid;
pub mod mcp_websearch;
pub mod question;
pub mod read;
pub mod registry;
pub mod shell;
pub mod shell_prompt;
pub mod skill;
pub mod todo;
// `tool/tool.rs` mirrors `reference/packages/opencode/src/tool/tool.ts`; the
// shared module path (`crate::tool::tool::Def`) is used throughout the crate.
#[allow(clippy::module_inception)]
pub mod tool;
pub mod webfetch;
pub mod websearch;
pub mod write;

pub mod apply_patch;
pub mod code_mode;
pub mod edit;
pub mod lsp;
pub mod plan;
pub mod task;
