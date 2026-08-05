//! oc-tui — ratatui port of the opencode terminal UI.
//!
//! Mirrors `reference/packages/tui/` + `reference/packages/session-ui/`.

pub mod app;
pub mod client;
pub mod components;
pub mod config;
pub mod keybind;
pub mod keymap;
pub mod local;
pub mod logo;
pub mod prompt;
pub mod sync;
pub mod theme;
pub mod types;
pub mod util;

pub use app::{run_async, App, TuiInput};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
