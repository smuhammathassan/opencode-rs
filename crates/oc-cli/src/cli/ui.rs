//! Terminal UI helpers.
//! From reference/packages/opencode/src/cli/ui.ts.

use std::io::Write;

use super::logo;

pub struct Style;

impl Style {
    pub const TEXT_HIGHLIGHT: &'static str = "\x1b[96m";
    pub const TEXT_HIGHLIGHT_BOLD: &'static str = "\x1b[96m\x1b[1m";
    pub const TEXT_DIM: &'static str = "\x1b[90m";
    pub const TEXT_DIM_BOLD: &'static str = "\x1b[90m\x1b[1m";
    pub const TEXT_NORMAL: &'static str = "\x1b[0m";
    pub const TEXT_NORMAL_BOLD: &'static str = "\x1b[1m";
    pub const TEXT_WARNING: &'static str = "\x1b[93m";
    pub const TEXT_WARNING_BOLD: &'static str = "\x1b[93m\x1b[1m";
    pub const TEXT_DANGER: &'static str = "\x1b[91m";
    pub const TEXT_DANGER_BOLD: &'static str = "\x1b[91m\x1b[1m";
    pub const TEXT_SUCCESS: &'static str = "\x1b[92m";
    pub const TEXT_SUCCESS_BOLD: &'static str = "\x1b[92m\x1b[1m";
    pub const TEXT_INFO: &'static str = "\x1b[94m";
    pub const TEXT_INFO_BOLD: &'static str = "\x1b[94m\x1b[1m";
}

/// Mirrors `UI.println`.
pub fn println(args: &[&str]) {
    print(args);
    let _ = std::io::stderr().write_all(b"\n");
}

/// Mirrors `UI.print`.
pub fn print(args: &[&str]) {
    let _ = std::io::stderr().write_all(args.join(" ").as_bytes());
}

/// Mirrors `UI.logo(pad?)`.
pub fn logo(pad: Option<&str>) -> String {
    logo::logo(pad)
}

/// Mirrors `UI.error(message)`: strips a leading "Error: " and prints a red
/// "Error: " banner.
pub fn error(message: &str) {
    let message = message.strip_prefix("Error: ").unwrap_or(message);
    println(&[&format!(
        "{}{} {}{}",
        Style::TEXT_DANGER_BOLD,
        "Error: ",
        Style::TEXT_NORMAL,
        message
    )]);
}

/// Mirrors `UI.empty()`.
pub fn empty() {
    println(&[Style::TEXT_NORMAL]);
}
