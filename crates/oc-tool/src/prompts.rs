//! Literal prompt resources, ported verbatim from
//! `reference/packages/opencode/src/tool/*.txt`.

/// `read.txt`
pub const READ: &str = include_str!("prompts/read.txt");

/// `write.txt`
pub const WRITE: &str = include_str!("prompts/write.txt");

/// `edit.txt`
pub const EDIT: &str = include_str!("prompts/edit.txt");

/// `glob.txt`
pub const GLOB: &str = include_str!("prompts/glob.txt");

/// `grep.txt`
pub const GREP: &str = include_str!("prompts/grep.txt");

/// `webfetch.txt`
pub const WEBFETCH: &str = include_str!("prompts/webfetch.txt");

/// `websearch.txt` — contains a `{{year}}` placeholder replaced at render time.
pub const WEBSEARCH: &str = include_str!("prompts/websearch.txt");

/// `task.txt`
pub const TASK: &str = include_str!("prompts/task.txt");

/// `todowrite.txt`
pub const TODOWRITE: &str = include_str!("prompts/todowrite.txt");

/// `question.txt`
pub const QUESTION: &str = include_str!("prompts/question.txt");

/// `skill.txt`
pub const SKILL: &str = include_str!("prompts/skill.txt");

/// `apply_patch.txt`
pub const APPLY_PATCH: &str = include_str!("prompts/apply_patch.txt");

/// `lsp.txt`
pub const LSP: &str = include_str!("prompts/lsp.txt");

/// `plan-exit.txt`
pub const PLAN_EXIT: &str = include_str!("prompts/plan-exit.txt");

/// `plan-enter.txt`
pub const PLAN_ENTER: &str = include_str!("prompts/plan-enter.txt");

/// `shell/shell.txt`
pub const SHELL: &str = include_str!("prompts/shell.txt");
