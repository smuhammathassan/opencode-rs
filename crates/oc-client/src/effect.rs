//! The `/effect` client surface.
//!
//! Mirrors `reference/packages/client/src/effect.ts`: the entrypoint re-exports
//! the canonical schema datatypes alongside the `OpenCode` client. In Rust the
//! promise and effect projections share a single implementation, so this module
//! only re-exports the datatypes; the generated-effect client
//! (`reference/packages/client/src/generated-effect/client.ts`) exposes the same
//! group/endpoint surface.

pub use crate::generated::*;
