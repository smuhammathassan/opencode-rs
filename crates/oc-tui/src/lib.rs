//! oc-tui — ratatui port of the opencode terminal UI.
//!
//! Mirrors `reference/packages/tui/` + `reference/packages/session-ui/`.

pub mod app;
pub mod client;
pub mod clipboard;
pub mod components;
pub mod config;
pub mod editor;
pub mod keybind;
pub mod keymap;
pub mod local;
pub mod logo;
pub mod prompt;
pub mod sync;
pub(crate) mod terminal;
pub mod theme;
pub mod types;
pub mod util;

pub use app::{run_async, App, TuiInput};

/// The version reported in the TUI: the reference version for drop-in parity,
/// shared via `oc-util`.
pub fn version() -> &'static str {
    oc_util::version::REFERENCE_VERSION
}
