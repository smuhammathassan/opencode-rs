//! Minimal markdown → styled-lines renderer for message text parts.
//!
//! The reference delegates markdown rendering to OpenTUI's `<markdown>` widget
//! (`reference/packages/tui/src/routes/session/index.tsx`, TextPart) with
//! `streaming`, `tableOptions.style = "grid"` and a `conceal` toggle. This
//! module is a ratatui equivalent: it renders a markdown string into styled
//! wrapped lines, with fenced-code concealment.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MdStyle {
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub fg: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdSpan {
    pub text: String,
    pub style: MdStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdLine {
    pub spans: Vec<MdSpan>,
}

impl MdLine {
    pub fn plain(text: impl Into<String>) -> Self {
        MdLine {
            spans: vec![MdSpan {
                text: text.into(),
                style: MdStyle::default(),
            }],
        }
    }
    pub fn styled(text: impl Into<String>, style: MdStyle) -> Self {
        MdLine {
            spans: vec![MdSpan {
                text: text.into(),
                style,
            }],
        }
    }
    pub fn empty() -> Self {
        MdLine { spans: Vec::new() }
    }
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
    pub fn is_empty(&self) -> bool {
        self.text().is_empty()
    }
}

/// Options controlling markdown rendering.
pub struct MarkdownOptions {
    /// Width (in cells) at which lines wrap.
    pub width: usize,
    /// Collapse fenced code blocks to a single line (default true in the TUI).
    pub conceal: bool,
    /// Base foreground for normal text.
    pub fg: Color,
    /// Color used for headings.
    pub heading: Color,
    /// Color used for code (inline and fenced).
    pub code: Color,
    /// Color for dim/quote/table text.
    pub muted: Color,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        MarkdownOptions {
            width: 80,
            conceal: true,
            fg: Color::White,
            heading: Color::Cyan,
            code: Color::Yellow,
            muted: Color::DarkGray,
        }
    }
}

/// Render `content` to wrapped styled lines.
pub fn render(content: &str, options: &MarkdownOptions) -> Vec<MdLine> {
    let mut out: Vec<MdLine> = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_buf: Vec<String> = Vec::new();
    let mut table_buf: Vec<String> = Vec::new();

    let flush_code =
        |out: &mut Vec<MdLine>, lang: &str, buf: &Vec<String>, options: &MarkdownOptions| {
            if buf.is_empty() {
                return;
            }
            if options.conceal {
                let n = buf.len();
                out.push(MdLine::styled(
                    format!(
                        "▍ {lang} · {n} line{} concealed",
                        if n == 1 { "" } else { "s" }
                    ),
                    MdStyle {
                        dim: true,
                        fg: Some(options.muted),
                        ..MdStyle::default()
                    },
                ));
            } else {
                let mut first = true;
                for line in buf {
                    let styled = if first {
                        MdSpan {
                            text: format!("▍ {lang} {line}"),
                            style: MdStyle {
                                fg: Some(options.code),
                                ..MdStyle::default()
                            },
                        }
                    } else {
                        MdSpan {
                            text: line.clone(),
                            style: MdStyle {
                                fg: Some(options.code),
                                ..MdStyle::default()
                            },
                        }
                    };
                    wrap_spans(vec![styled], options.width, options).extend_into(out);
                    first = false;
                }
            }
        };

    for raw in content.split('\n') {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();

        if !in_code {
            if trimmed.starts_with("```") {
                in_code = true;
                code_lang = trimmed.trim_start_matches("```").trim().to_string();
                code_buf.clear();
                continue;
            }
            // Grid table: `| a | b |` rows plus separator rows.
            if trimmed.starts_with('|') && trimmed.ends_with('|') {
                table_buf.push(trimmed.to_string());
                continue;
            }
            if !table_buf.is_empty() {
                out.extend(render_table(&table_buf, options));
                table_buf.clear();
            }
            render_block_line(line, options, &mut out);
        } else if trimmed.starts_with("```") {
            flush_code(&mut out, &code_lang, &code_buf, options);
            in_code = false;
        } else {
            code_buf.push(line.to_string());
        }
    }
    if in_code {
        flush_code(&mut out, &code_lang, &code_buf, options);
    }
    if !table_buf.is_empty() {
        out.extend(render_table(&table_buf, options));
    }
    out
}

trait ExtendLines {
    fn extend_into(self, out: &mut Vec<MdLine>);
}
impl ExtendLines for Vec<MdLine> {
    fn extend_into(self, out: &mut Vec<MdLine>) {
        out.extend(self);
    }
}

fn render_block_line(line: &str, options: &MarkdownOptions, out: &mut Vec<MdLine>) {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();

    let (prefix, style) = if let Some(rest) = heading_level(trimmed) {
        (
            Some(rest),
            MdStyle {
                bold: true,
                fg: Some(options.heading),
                ..MdStyle::default()
            },
        )
    } else if let Some(rest) = trimmed.strip_prefix(">") {
        (
            Some(rest.trim_start().to_string()),
            MdStyle {
                dim: true,
                fg: Some(options.muted),
                ..MdStyle::default()
            },
        )
    } else if trimmed.starts_with('-') || trimmed.starts_with('*') || trimmed.starts_with('+') {
        let rest = trimmed[1..].trim_start();
        (Some(format!("• {rest}")), MdStyle::default())
    } else if trimmed.starts_with("---") || trimmed.starts_with("***") {
        (
            Some("─".repeat(options.width.saturating_sub(indent))),
            MdStyle {
                dim: true,
                fg: Some(options.muted),
                ..MdStyle::default()
            },
        )
    } else {
        (Some(trimmed.to_string()), MdStyle::default())
    };

    if let Some(rest) = prefix {
        let mut spans = inline(&rest, style, options);
        if indent > 0 && spans.is_empty() {
            spans.push(MdSpan {
                text: String::new(),
                style: MdStyle::default(),
            });
        }
        wrap_spans(spans, options.width.saturating_sub(indent), options).extend_into(out);
    } else {
        out.push(MdLine::empty());
    }
}

fn heading_level(trimmed: &str) -> Option<String> {
    let mut hashes = 0;
    for c in trimmed.chars() {
        if c == '#' {
            hashes += 1;
            if hashes > 6 {
                return None;
            }
        } else if c == ' ' {
            return Some(trimmed[hashes..].trim_start().to_string());
        } else {
            return None;
        }
    }
    None
}

/// Parse inline markdown constructs (bold, italic, inline code, links) into spans.
fn inline(text: &str, base: MdStyle, options: &MarkdownOptions) -> Vec<MdSpan> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();

    let flush = |buf: &mut String, spans: &mut Vec<MdSpan>| {
        if !buf.is_empty() {
            spans.push(MdSpan {
                text: std::mem::take(buf),
                style: base,
            });
        }
    };

    while i < chars.len() {
        let c = chars[i];
        // Inline code `` `x` ``
        if c == '`' {
            let mut end = i + 1;
            let mut code = String::new();
            while end < chars.len() && chars[end] != '`' {
                code.push(chars[end]);
                end += 1;
            }
            if end < chars.len() {
                flush(&mut buf, &mut spans);
                spans.push(MdSpan {
                    text: code,
                    style: MdStyle {
                        fg: Some(options.code),
                        ..base
                    },
                });
                i = end + 1;
                continue;
            }
        }
        // Bold **x** / __x__ and italic *x* / _x_
        if c == '*' || c == '_' {
            let delim = c;
            if i + 1 < chars.len() && chars[i + 1] == delim {
                if let Some((inner, next)) = scan_delimited_pair(&chars, i + 2, delim) {
                    flush(&mut buf, &mut spans);
                    spans.extend(inline(&inner, MdStyle { bold: true, ..base }, options));
                    i = next;
                    continue;
                }
            } else if let Some((inner, next)) = scan_delimited(&chars, i + 1, delim) {
                flush(&mut buf, &mut spans);
                spans.extend(inline(
                    &inner,
                    MdStyle {
                        italic: true,
                        ..base
                    },
                    options,
                ));
                i = next;
                continue;
            }
        }
        // Link [text](url)
        if c == '[' {
            if let Some((label, next)) = scan_delimited(&chars, i + 1, ']') {
                let after = next;
                if after + 1 < chars.len() && chars[after] == '(' {
                    if let Some((_, url_end)) = scan_delimited(&chars, after + 1, ')') {
                        flush(&mut buf, &mut spans);
                        spans.extend(inline(&label, base, options));
                        i = url_end;
                        continue;
                    }
                }
            }
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut spans);
    spans
}

/// Scan for a closing `delim` starting at `start`.
/// Returns the inner text and the index after the closing delimiter.
fn scan_delimited(chars: &[char], start: usize, delim: char) -> Option<(String, usize)> {
    let mut inner = String::new();
    let mut i = start;
    while i < chars.len() {
        if chars[i] == delim {
            return Some((inner, i + 1));
        }
        inner.push(chars[i]);
        i += 1;
    }
    None
}

/// Scan for a closing doubled delimiter (`**` / `__`). Returns the inner text
/// and the index after the closing pair.
fn scan_delimited_pair(chars: &[char], start: usize, delim: char) -> Option<(String, usize)> {
    let mut inner = String::new();
    let mut i = start;
    while i < chars.len() {
        if chars[i] == delim {
            if i + 1 < chars.len() && chars[i + 1] == delim {
                return Some((inner, i + 2));
            }
            inner.push(delim);
            i += 1;
        } else {
            inner.push(chars[i]);
            i += 1;
        }
    }
    None
}

/// Render a grid-style markdown table.
fn render_table(rows: &[String], options: &MarkdownOptions) -> Vec<MdLine> {
    let mut cells: Vec<Vec<String>> = Vec::new();
    for row in rows {
        let inner = row.trim().trim_start_matches('|').trim_end_matches('|');
        cells.push(inner.split('|').map(|c| c.trim().to_string()).collect());
    }
    if cells.is_empty() {
        return Vec::new();
    }
    let cols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return Vec::new();
    }
    let mut widths = vec![0usize; cols];
    for row in &cells {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.chars().count());
        }
    }
    let cap = options.width.saturating_sub(3 * (cols + 1));
    if cap > 0 {
        let total: usize = widths.iter().sum();
        if total > cap {
            // Shrink proportionally so the table fits.
            let mut budget = cap;
            let mut cols_budget = cols;
            for w in &mut widths {
                let share = (*w * cap) / total.max(1);
                let take = share.min(budget / cols_budget.max(1)).min(*w);
                *w = take.max(1);
                budget = budget.saturating_sub(take);
                cols_budget -= 1;
            }
        }
    }

    let border = |_sep: &str| -> String {
        let mut line = String::new();
        line.push('+');
        for w in &widths {
            line.push_str(&"-".repeat(*w + 2));
            line.push('+');
        }
        line
    };

    let mut out = Vec::new();
    out.push(MdLine::styled(
        border(""),
        MdStyle {
            dim: true,
            fg: Some(options.muted),
            ..MdStyle::default()
        },
    ));
    let header = cells.first();
    let mut row_start = 0;
    for (ri, row) in cells.iter().enumerate() {
        let is_separator = row.iter().all(|c| {
            let t = c.trim();
            t.is_empty() || (t.starts_with('-') && t.ends_with('-'))
        });
        if is_separator {
            continue;
        }
        let mut spans = Vec::new();
        for (ci, w) in widths.iter().enumerate() {
            let cell = row.get(ci).cloned().unwrap_or_default();
            spans.push(MdSpan {
                text: format!("│ {cell:<w$} "),
                style: if ri == 0 {
                    MdStyle {
                        bold: true,
                        fg: Some(options.fg),
                        ..MdStyle::default()
                    }
                } else {
                    MdStyle {
                        fg: Some(options.muted),
                        ..MdStyle::default()
                    }
                },
            });
        }
        spans.push(MdSpan {
            text: "│".to_string(),
            style: MdStyle::default(),
        });
        out.push(MdLine { spans });
        row_start = ri + 1;
    }
    let _ = header;
    let _ = row_start;
    out.push(MdLine::styled(
        border(""),
        MdStyle {
            dim: true,
            fg: Some(options.muted),
            ..MdStyle::default()
        },
    ));
    out
}

/// Wrap spans to `width`, splitting text at word boundaries. Long words and
/// code spans are broken at width to guarantee each line fits.
fn wrap_spans(spans: Vec<MdSpan>, width: usize, options: &MarkdownOptions) -> Vec<MdLine> {
    if width == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut current: Vec<MdSpan> = Vec::new();
    let mut current_len = 0usize;

    let push_line = |current: &mut Vec<MdSpan>, out: &mut Vec<MdLine>| {
        let line = MdLine {
            spans: std::mem::take(current),
        };
        if line.spans.is_empty() {
            out.push(MdLine::empty());
        } else {
            out.push(line);
        }
    };

    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        // Tokenize into words but keep code spans unbroken.
        if span.style.fg == Some(options.code) {
            // Treat code spans atomically, breaking at width if needed.
            let mut text = span.text.as_str();
            while !text.is_empty() {
                let take = available(text, width.saturating_sub(current_len));
                if take == 0 && current_len > 0 {
                    push_line(&mut current, &mut out);
                    current_len = 0;
                    continue;
                }
                if take == 0 {
                    // Single character wider than the terminal; drop to avoid an infinite loop.
                    break;
                }
                let (piece, rest) = split_at_width(text, take);
                current.push(MdSpan {
                    text: piece,
                    style: span.style,
                });
                current_len += take;
                text = rest;
                if !text.is_empty() && current_len >= width {
                    push_line(&mut current, &mut out);
                    current_len = 0;
                }
            }
            continue;
        }

        let words = tokenize_words(&span.text);
        for (word, leading_space) in words {
            let word_w = unicode_width(&word);
            if word_w == 0 {
                continue;
            }
            let space_w = if leading_space && current_len > 0 {
                1
            } else {
                0
            };
            if current_len + space_w + word_w > width && current_len > 0 {
                push_line(&mut current, &mut out);
                current_len = 0;
            }
            if leading_space && current_len > 0 {
                current.push(MdSpan {
                    text: " ".to_string(),
                    style: span.style,
                });
                current_len += 1;
            }
            if word_w > width && current_len == 0 {
                // Long word: hard-break.
                let mut rest = word.as_str();
                while !rest.is_empty() {
                    let take = available(rest, width.saturating_sub(current_len));
                    if take == 0 {
                        break;
                    }
                    let (piece, tail) = split_at_width(rest, take);
                    current.push(MdSpan {
                        text: piece,
                        style: span.style,
                    });
                    current_len += take;
                    rest = tail;
                    if !rest.is_empty() && current_len >= width {
                        push_line(&mut current, &mut out);
                        current_len = 0;
                    }
                }
            } else {
                current.push(MdSpan {
                    text: word.clone(),
                    style: span.style,
                });
                current_len += word_w;
            }
        }
    }
    if !current.is_empty() || out.is_empty() {
        out.push(MdLine { spans: current });
    }
    out
}

/// Split text into (word, had_leading_space) pairs.
fn tokenize_words(text: &str) -> Vec<(String, bool)> {
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut current = String::new();
    let mut saw_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !current.is_empty() {
                words.push((std::mem::take(&mut current), saw_space));
                saw_space = true;
            } else {
                saw_space = true;
            }
        } else {
            if saw_space && !current.is_empty() {
                words.push((std::mem::take(&mut current), true));
                saw_space = false;
            }
            current.push(c);
        }
    }
    if !current.is_empty() {
        words.push((current, saw_space));
    } else if saw_space {
        // Trailing whitespace preserved as a single space.
        words.push((" ".to_string(), false));
    }
    words
}

fn available(text: &str, max: usize) -> usize {
    if max == 0 {
        return 0;
    }
    let mut width = 0usize;
    for c in text.chars() {
        let w = c.width();
        if width + w > max {
            break;
        }
        width += w;
    }
    width
}

fn split_at_width(text: &str, width: usize) -> (String, &str) {
    let mut cur = 0usize;
    let mut idx = 0usize;
    for (byte_idx, c) in text.char_indices() {
        let w = c.width();
        if cur + w > width {
            return (text[..byte_idx].to_string(), &text[byte_idx..]);
        }
        cur += w;
        idx = byte_idx + c.len_utf8();
    }
    (text.to_string(), &text[idx..])
}

trait Width {
    fn width(&self) -> usize;
}
impl Width for char {
    fn width(&self) -> usize {
        unicode_width::UnicodeWidthChar::width(*self)
            .unwrap_or(0)
            .max(if *self == '\n' { 0 } else { 1 })
    }
}
fn unicode_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to_text(content: &str, width: usize) -> Vec<String> {
        render(
            content,
            &MarkdownOptions {
                width,
                ..MarkdownOptions::default()
            },
        )
        .into_iter()
        .map(|l| l.text())
        .collect()
    }

    #[test]
    fn renders_headings_and_bold() {
        let lines = render_to_text("# Hello **world**", 80);
        assert_eq!(lines[0], "Hello world");
        assert!(lines[0].contains("world"));
        let rendered = render("# Hello **world**", &MarkdownOptions::default());
        assert!(rendered[0].spans[0].style.bold);
    }

    #[test]
    fn inline_code_is_kept() {
        let lines = render_to_text("Use `cargo build` now", 80);
        assert_eq!(lines[0], "Use cargo build now");
    }

    #[test]
    fn code_block_concealed_by_default() {
        let content = "```rust\nfn main() {}\n```";
        let lines = render_to_text(content, 80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("concealed"));
    }

    #[test]
    fn code_block_expanded_when_not_concealed() {
        let content = "```rust\nfn main() {}\n```";
        let options = MarkdownOptions {
            width: 80,
            conceal: false,
            ..MarkdownOptions::default()
        };
        let lines = render(content, &options)
            .into_iter()
            .map(|l| l.text())
            .collect::<Vec<_>>();
        assert_eq!(lines, vec!["▍ rust fn main() {}"]);
    }

    #[test]
    fn wraps_long_text() {
        let content = "word ".repeat(30);
        let lines = render_to_text(&content, 20);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(line.chars().count() <= 21);
        }
    }

    #[test]
    fn grid_table() {
        let content = "| a | b |\n|---| ---|\n| 1 | 2 |";
        let rendered = render_to_text(content, 80);
        assert!(rendered[0].starts_with('+'));
        assert!(rendered.iter().any(|l| l.contains("a") && l.contains("b")));
    }
}
