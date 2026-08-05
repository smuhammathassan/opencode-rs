//! In-process JavaScript execution for plugins.
//!
//! The reference spawns a Bun subprocess to run JS plugins; this crate runs
//! them in-process on QuickJS (via `libquickjs-sys`). The `quick-js` crate is
//! too limited for the plugin host (no job-queue access, no object readback),
//! so `runtime` is a thin, documented unsafe wrapper over the sys bindings.
//! All `unsafe` in this crate lives in `runtime.rs`.

pub mod runtime;
pub mod transpile;
pub mod value;

pub use runtime::{Callback, Runtime};
pub use value::{JsError, JsValue};
