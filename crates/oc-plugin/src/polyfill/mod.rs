//! The embedded `@opencode-ai/plugin` polyfill runtime.
//!
//! `runtime.js` is evaluated once per QuickJS context and provides the module
//! registry, import shims, the plugin API surface, and the bridge protocol to
//! the Rust host.

/// The JS runtime source, embedded so the binary needs no external files.
pub const RUNTIME_SOURCE: &str = include_str!("runtime.js");

pub fn runtime_source() -> &'static str {
    RUNTIME_SOURCE
}
