//! Mirrors `reference/packages/opencode/src/cli/`.

pub mod args;
pub mod auth;
pub mod bootstrap;
pub mod cmd;
pub mod context;
pub mod effect_cmd;
pub mod error;
pub mod heap;
pub mod logo;
pub mod models_dev;
pub mod network;
pub mod paths;
pub mod ui;
pub mod upgrade;

/// The opencode release this port mirrors.
/// From reference/packages/opencode/package.json.
pub const VERSION: &str = crate::VERSION;
