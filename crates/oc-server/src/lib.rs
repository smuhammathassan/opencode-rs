#![allow(clippy::type_complexity)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::unnecessary_lazy_evaluations)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::double_ended_iterator_last)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::map_identity)]
#![allow(clippy::needless_return)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::all)]
//! oc-server — HTTP API, SSE event stream, and WebSocket server.
//!
//! A 1:1 Rust port of the opencode HTTP server:
//! - `reference/packages/server/` (v2 `/api` surface: `api.ts`, `routes.ts`,
//!   `handlers/`, `cors.ts`, `location.ts`, `pty-environment.ts`, `middleware/`)
//! - `reference/packages/opencode/src/server/` (listener, projectors, global
//!   lifecycle, auth, mdns, proxy-util, shared/)

pub mod auth;
pub mod builtin_auth;
pub mod cors;
pub mod errors;
pub mod event;
pub mod global_lifecycle;
pub mod handlers;
pub mod init_projectors;
pub mod instance_handlers;
pub mod location;
pub mod mdns;
pub mod middleware;
pub mod openapi;
pub(crate) mod plugin_auth;
pub(crate) mod plugin_registry;
pub mod projectors;
pub mod proxy_util;
pub mod pty_environment;
pub mod route;
pub mod router;
pub mod runner;
pub mod schema;
pub mod server;
pub(crate) mod share;
pub mod shared;
pub mod sse;
pub mod state;
pub mod web;

pub use state::AppState;

/// The version reported by `/global/health`, `/generate`, and `/debug/info`.
/// Mirrors `InstallationVersion` in the reference: the upstream opencode
/// version this port tracks (`REFERENCE_VERSION`), not the crate's package
/// version.
pub fn version() -> &'static str {
    oc_util::version::REFERENCE_VERSION
}

/// Build a ready-to-serve router with default settings. Convenience for embedders;
/// production wiring goes through [`server::listen`].
pub fn app(
    auth: auth::AuthConfig,
    cors: cors::CorsOptions,
    location: location::Location,
) -> axum::Router {
    let state = AppState::new(auth, cors, location);
    router::build(state)
}
