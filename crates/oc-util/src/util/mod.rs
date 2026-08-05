//! Mirrors `packages/opencode/src/util/` (with re-exports that the reference
//! pulls from `packages/tui/src/util/` and `packages/core/src/util/`).

pub mod archive;
pub mod bom;
pub mod data_url;
pub mod defer;
pub mod error;
pub mod filesystem;
pub mod html;
pub mod iife;
pub mod lazy;
pub mod local_context;
pub mod locale;
pub mod media;
pub mod process;
pub mod proxy_env;
pub mod queue;
pub mod record;
pub mod rpc;
pub mod signal;
pub mod timeout;
pub mod token;
pub mod wildcard;
