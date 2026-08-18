//! Control plane: workspaces, adapters, and session relocation.
//!
//! Mirrors `src/control-plane` + `core/control-plane` of opencode v1.18.13.

pub mod adapters;
pub mod deps;
pub mod dev;
pub mod global_bus;
pub mod memory_transport;
pub mod move_session;
pub mod slug;
pub mod sse;
pub mod sync_api;
pub mod types;
pub mod util;
pub mod workspace;
pub mod workspace_adapter_runtime;
pub mod workspace_context;
pub mod workspace_events;
pub mod workspace_sql;
