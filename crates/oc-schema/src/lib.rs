//! `oc-schema` — 1:1 Rust port of `@opencode-ai/schema` (reference v1.18.13).
//!
//! Serde types with exact JSON field names, defaults, nullability, and union
//! discrimination matching the reference zod schemas in
//! `reference/packages/schema/src/`.

pub mod agent;
pub mod catalog;
pub mod command;
pub mod connection;
pub mod credential;
pub mod durable_event_manifest;
pub mod event;
pub mod event_manifest;
pub mod file_diff;
pub mod filesystem;
pub mod filesystem_watcher;
pub mod ide_event;
pub mod identifier;
pub mod installation_event;
pub mod integration;
pub mod integration_id;
pub mod legacy_event;
pub mod llm;
pub mod location;
pub mod lsp_event;
pub mod mcp_event;
pub mod model;
pub mod models_dev;
pub mod permission;
pub mod permission_saved;
pub mod permission_v1;
pub mod plugin;
pub mod project;
pub mod project_copy;
pub mod project_directories;
pub mod project_id;
pub mod prompt;
pub mod prompt_input;
pub mod provider;
pub mod pty;
pub mod pty_ticket;
pub mod question;
pub mod question_v1;
pub mod reference;
pub mod revert;
pub mod schema;
pub mod server_event;
pub mod session;
pub mod session_compaction_event;
pub mod session_delivery;
pub mod session_event;
pub mod session_id;
pub mod session_input;
pub mod session_message;
pub mod session_status_event;
pub mod session_todo;
pub mod session_v1;
pub mod skill;
pub mod tui_event;
pub mod v1;
pub mod vcs_event;
pub mod workspace;
pub mod workspace_event;
pub mod workspace_id;
pub mod worktree_event;

// `export { Prompt, Source, FileAttachment, AgentAttachment } from "./prompt"`
pub use prompt::{AgentAttachment, FileAttachment, Prompt, Source};

// `export * from "./schema"`
pub use schema::{AbsolutePath, DateTimeUtc, Finite, NonNegativeInt, PositiveInt, RelativePath};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
