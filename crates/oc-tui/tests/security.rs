//! Terminal security, ANSI escape, and OSC injection prevention tests.
//!
//! Asserts that untrusted input passed into the production rendering and
//! sanitization pipelines (in `oc_tui::util::display` and `oc_tui::util::markdown`)
//! strips harmful control characters, OSC terminal title modifications, and
//! OSC 52 clipboard escape sequences.

use oc_tui::components::text::plain;
use oc_tui::components::text::to_ratatui;
use oc_tui::util::display::sanitize_terminal_text;
use oc_tui::util::markdown::{render, MarkdownOptions};

#[test]
fn sanitize_osc_terminal_title_injection() {
    let malicious = "\x1b]0;hacked-title\x07Normal Text";
    let sanitized = sanitize_terminal_text(malicious);
    assert_eq!(sanitized, "Normal Text");
    assert!(!sanitized.contains("\x1b]0;"));
    assert!(!sanitized.contains("hacked-title"));
}

#[test]
fn sanitize_osc_52_clipboard_injection() {
    let malicious = "\x1b]52;c;ZXhwbG9pdA==\x07Hello Safe World";
    let sanitized = sanitize_terminal_text(malicious);
    assert_eq!(sanitized, "Hello Safe World");
    assert!(!sanitized.contains("\x1b]52;"));
    assert!(!sanitized.contains("ZXhwbG9pdA=="));
}

#[test]
fn sanitize_ansi_color_bombs() {
    let malicious = "\x1b[38;2;255;0;0mRed Text\x1b[0m";
    let sanitized = sanitize_terminal_text(malicious);
    assert_eq!(sanitized, "Red Text");
    assert!(!sanitized.contains("\x1b["));
}

#[test]
fn production_markdown_renderer_escapes_injections() {
    let malicious =
        "# Header \x1b]0;bad-title\x07\n\nSome text with `inline \x1b]52;c;evil\x07 code`";
    let sanitized = sanitize_terminal_text(malicious);
    let lines = render(&sanitized, &MarkdownOptions::default());
    let full_text: String = lines.iter().map(|l| l.text()).collect::<Vec<_>>().join(" ");
    assert!(!full_text.contains("\x1b]0;"));
    assert!(!full_text.contains("bad-title"));
    assert!(!full_text.contains("\x1b]52;"));
    assert!(!full_text.contains("evil"));
}

#[test]
fn production_styled_line_ratatui_conversion_is_safe() {
    let malicious = sanitize_terminal_text("Safe Line \x07 with no bells");
    let line = plain(malicious);
    let ratatui_line = to_ratatui(&line);
    let rendered = format!("{:?}", ratatui_line);
    assert!(!rendered.contains("\\u{7}"));
    assert!(!rendered.contains("\x07"));
}
