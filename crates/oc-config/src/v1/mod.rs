// V1 config schema types (opencode.json document).
//
// From reference/packages/core/src/v1/config/

pub mod agent;
pub mod attachment;
pub mod command;
pub mod config;
pub mod formatter;
pub mod layout;
pub mod lsp;
pub mod mcp;
pub mod permission;
pub mod plugin;
pub mod provider;
pub mod server;
pub mod skills;

pub use config::{
    AutoUpdate, Compaction, Enterprise, Experimental, Info, LogLevel, Share, Skills, ToolOutput,
    Watcher,
};
pub use permission::{Action, Info as PermissionInfo, Rule};
