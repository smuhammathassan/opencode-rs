//! oc-plugin: the plugin host for opencode-rs.
//!
//! Mirrors the plugin surface of the reference opencode v1.18.13:
//! - `packages/plugin` — the public `@opencode-ai/plugin` API polyfilled into
//!   in-process QuickJS
//! - `packages/core/src/plugin` — the host (hooks registry, trigger)
//! - `packages/opencode/src/plugin` — loading, installation, metadata
//!
//! The core divergence: the reference spawns a Bun subprocess to run JS
//! plugins; this crate runs them in-process on QuickJS (`libquickjs-sys`) for
//! the memory/CPU goal. JS is evaluated synchronously; promises are driven by
//! pumping the QuickJS job queue, and all cross-boundary data is JSON strings.
//! See `js::runtime` for the (confined) unsafe FFI and `polyfill` for the API.

pub mod bridge;
pub mod config;
pub mod host;
pub mod install;
pub mod js;
pub mod jsonc;
pub mod loader;
pub mod meta;
pub mod npm;
pub mod paths;
pub mod polyfill;
pub mod shared;

pub use host::{LoadedPlugin, LoadedSummary, NoopHost, PluginBuilder, PluginHost, ToolInfo};
pub use js::{JsError, JsValue};
pub use loader::ModuleResolver;
pub use paths::GlobalPaths;

/// The running opencode version, used for plugin compatibility checks.
/// TODO(integration): share the version from the workspace package metadata.
pub const OPENCODE_VERSION: &str = "1.18.13";
