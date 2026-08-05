// Core config v2 schema types (`ConfigV2.*`).
//
// From reference/packages/core/src/config/

pub mod agent;
pub mod attachments;
pub mod command;
pub mod compaction;
pub mod experimental;
pub mod formatter;
pub mod lsp;
pub mod markdown;
pub mod mcp;
pub mod permission;
pub mod plugin;
pub mod provider;
pub mod reference;
pub mod tool_output;
pub mod watcher;

pub use agent::{Color, Info as Agent, Mode as AgentMode, NamedColor, Request as AgentRequest};
pub use attachments::{Image as AttachmentImage, Info as Attachments};
pub use command::Info as Command;
pub use compaction::{Info as Compaction, Keep};
pub use experimental::{Effect as PolicyEffect, Experimental, Policy, PolicyAction};
pub use formatter::{Entry as FormatterEntry, Info as Formatter};
pub use lsp::{Entry as LspEntry, Info as Lsp, Server as LspServer};
pub use mcp::{Info as Mcp, Local as McpLocal, OAuth as McpOAuth, Remote as McpRemote, Server as McpServer, Timeout as McpTimeout};
pub use permission::{Effect as PermissionEffect, Rule as PermissionRule, Ruleset};
pub use plugin::{Entry as PluginEntry, Plugin, Plugins};
pub use provider::{Cache, Capabilities, Cost, CostOrArray, CostTier, Info as Provider, Limit, Model, ModelApi, ModelApiTagged, ModelRequest, ModelVariant, ProviderApi, Request as ProviderRequest};
pub use reference::{Entry as ReferenceEntry, Git as ReferenceGit, Info as Reference, Local as ReferenceLocal};
pub use tool_output::Info as ToolOutput;
pub use watcher::Info as Watcher;
