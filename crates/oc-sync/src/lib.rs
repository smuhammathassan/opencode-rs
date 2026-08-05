//! oc-sync — event-sourcing/replay of session events and remote workspaces.
//!
//! Mirrors `src/sync` + `src/control-plane` + `core/control-plane` of opencode
//! v1.18.13 (reference/packages/opencode/src/ and reference/packages/core/src/):
//!
//! - `sync`: the sync event schema (`EventID`), the `EventV2` durable event
//!   store with cursor ordering, and the `event` / `event_sequence` SQL DDL.
//! - `control_plane`: workspace types, adapters, the workspace runtime, session
//!   move/warp, and the `workspace` SQL DDL.

pub mod control_plane;
pub mod sync;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
