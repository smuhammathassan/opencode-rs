#![allow(
    clippy::field_reassign_with_default,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::module_inception,
    clippy::needless_range_loop,
    clippy::if_same_then_else,
    clippy::no_effect,
    clippy::manual_strip
)]
//! `oc-tool` — a 1:1 Rust port of the opencode v1.18.13 tool machinery.
//!
//! Two tool engines are ported:
//! - `core` — the V2 core engine (`reference/packages/core/src/tool/`), used by
//!   the V2 session runtime.
//! - `tool` — the opencode (V1) engine (`reference/packages/opencode/src/tool/`),
//!   used by the mainline session runner.
//!
//! Shared wire types live in `model`; prompt resources are verbatim copies of
//! the reference `.txt` files in `prompts/`.

pub mod base64;
pub mod checksum;
pub mod core;
pub mod diff;
pub mod http;
pub mod jsonschema;
pub mod mime;
pub mod model;
pub mod patch;
pub mod prompts;
pub mod ripgrep;
pub mod schema;
pub mod shell;
pub mod tool;
pub mod truncate;
pub mod util;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
