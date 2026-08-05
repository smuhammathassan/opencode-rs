//! ASCII wordmark / logo rendering.
//! From reference/packages/opencode/src/cli/logo.ts and
//! reference/packages/opencode/src/cli/ui.ts (`wordmark`).

use std::io::IsTerminal;

/// The `left`/`right` glyph grids used for the color TTY logo.
/// From reference/packages/tui/src/logo.ts.
pub const GLYPHS_LEFT: [&str; 4] = [
    "                   ",
    "█▀▀█ █▀▀█ █▀▀█ █▀▀▄",
    "█__█ █__█ █^^^ █__█",
    "▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀~~▀",
];

pub const GLYPHS_RIGHT: [&str; 4] = [
    "             ▄     ",
    "█▀▀▀ █▀▀█ █▀▀█ █▀▀█",
    "█___ █__█ █__█ █^^^",
    "▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀",
];

/// Plain wordmark used for non-TTY output.
/// From reference/packages/opencode/src/cli/ui.ts.
pub const WORDMARK: [&str; 4] = [
    "⠀                                ▄     ",
    "█▀▀█ █▀▀█ █▀▀█ █▀▀▄ █▀▀▀ █▀▀█ █▀▀█ █▀▀█",
    "█  █ █  █ █▀▀▀ █  █ █    █  █ █  █ █▀▀▀",
    "▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀  ▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀",
];

fn draw(line: &str, fg: &str, shadow: &str, bg: &str) -> String {
    let mut out = String::new();
    let reset = "\x1b[0m";
    for ch in line.chars() {
        match ch {
            '_' => {
                out.push_str(bg);
                out.push(' ');
                out.push_str(reset);
            }
            '^' => {
                out.push_str(fg);
                out.push_str(bg);
                out.push('▀');
                out.push_str(reset);
            }
            '~' => {
                out.push_str(shadow);
                out.push('▀');
                out.push_str(reset);
            }
            ' ' => out.push(' '),
            _ => {
                out.push_str(fg);
                out.push(ch);
                out.push_str(reset);
            }
        }
    }
    out
}

/// Render the logo, optionally padded. Mirrors `UI.logo(pad?)`.
pub fn logo(pad: Option<&str>) -> String {
    if !std::io::stdout().is_terminal() && !std::io::stderr().is_terminal() {
        let mut out = String::new();
        for row in WORDMARK {
            if let Some(pad) = pad {
                out.push_str(pad);
            }
            out.push_str(row);
            out.push('\n');
        }
        return out.trim_end().to_string();
    }

    let reset = "\x1b[0m";
    let left = ("\x1b[90m", "\x1b[38;5;235m", "\x1b[48;5;235m");
    let right = (reset, "\x1b[38;5;238m", "\x1b[48;5;238m");
    let mut out = String::new();
    for (index, row) in GLYPHS_LEFT.iter().enumerate() {
        if let Some(pad) = pad {
            out.push_str(pad);
        }
        out.push_str(&draw(row, left.0, left.1, left.2));
        out.push(' ');
        let other = GLYPHS_RIGHT.get(index).copied().unwrap_or("");
        out.push_str(&draw(other, right.0, right.1, right.2));
        out.push('\n');
    }
    out.trim_end().to_string()
}
