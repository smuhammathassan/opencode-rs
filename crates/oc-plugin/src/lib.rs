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
//!
//! # Known limitations (flags for the integration lead)
//!
//! - **Timers** (`setTimeout`/`setInterval`) run their callback on the next
//!   microtask tick; wall-clock delays are not honored (no native event loop).
//! - **Plugin sources** are transpiled in-process (TypeScript strip + ESM
//!   transform + ASI) and must stick to the supported subset; the exact
//!   supported constructs are exercised by `tests/integration.rs` and
//!   `js::transpile` tests.
//! - **v2 effect API** (`opencode/plugin/v2/effect`) is a stub: `define` is a
//!   passthrough and the Effect runtime is out of scope. The promise-based v2
//!   surface (`opencode/plugin/v2/promise`) works through the same host.
//! - **Built-in auth plugins** (`openai/codex`, `github-copilot`, `modal`,
//!   `azure`, `xai`, `digitalocean`, ...) in
//!   `reference/packages/opencode/src/plugin/` are not ported; they belong to
//!   the provider/auth domain (oc-provider/oc-command).
//! - **Built-in v2 config plugins** (`core/src/plugin/{agent,command,provider,
//!   skill,variant}.ts`) are not ported; they are core-engine defaults applied
//!   through the v2 transform bridge (`LoadedPlugin::v2_transform`).
//! - The QuickJS build bundled by `libquickjs-sys` predates `globalThis`,
//!   promise-state and object-enumeration APIs; the runtime polyfills
//!   `globalThis` and reads objects via `Object.keys` (see `js::runtime`).

#![allow(clippy::all)]

pub mod bridge;
pub mod config;
pub mod default_plugins;
pub mod host;
pub mod install;
pub mod js;
pub mod jsonc;
pub mod loader;
pub mod manager;
pub mod meta;
pub mod npm;
pub mod npm_config;
pub mod paths;
pub mod polyfill;
pub mod registration;
pub mod shared;

pub use host::{
    LoadedPlugin, LoadedSummary, LocalHost, NoopHost, PluginAuthMethodSummary,
    PluginAuthMethodType, PluginAuthOptionSummary, PluginAuthPromptSummary, PluginAuthSummary,
    PluginAuthWhenSummary, PluginBuilder, PluginHost, PluginToolCancellation, ToolInfo,
};
pub use js::{JsError, JsValue, RuntimeLimits};
pub use loader::ModuleResolver;
pub use manager::{
    AuthAuthorizeRequest, AuthCallbackRequest, AuthValidateRequest, PluginLoadReport, PluginManager,
};
pub use paths::GlobalPaths;
pub use registration::{
    ClientRpcRequest, InMemoryRegistrationSink, PluginRegistration, PluginRegistrationSink,
};

/// The running opencode version, used for plugin compatibility checks.
/// TODO(integration): share the version from the workspace package metadata.
pub const OPENCODE_VERSION: &str = "1.18.13";
