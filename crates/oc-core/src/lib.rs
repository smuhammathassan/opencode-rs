//! oc-core — core engine glue.
//!
//! Rust port of `reference/packages/core/src/` (opencode v1.18.13): the async
//! event bus, background job runner, ID generation, git wrapper, file mutation
//! types, project detection, credential storage, and the service graph that
//! ties them together.
//!
//! Field names deliberately match the reference's camelCase JSON wire format.
#![allow(non_snake_case)]

pub mod account;
pub mod agent;
pub mod background_job;
pub mod bus;
pub mod catalog;
pub mod command;
pub mod context;
pub mod credential;
pub mod durable;
pub mod event;
pub mod file;
pub mod file_mutation;
pub mod fs_util;
pub mod git;
pub mod id;
pub mod identifier;
pub mod ids;
pub mod installation;
pub mod integration;
pub mod keyed_mutex;
pub mod location;
pub mod model;
pub mod policy;
pub mod process;
pub mod project;
pub mod provider;
pub mod schema;
pub mod state;
pub mod util;

pub use context::Services;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
