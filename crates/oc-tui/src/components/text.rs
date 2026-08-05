//! Styled line primitives shared by renderers.
//!
//! Renderers produce `StyledLine`s (display-width aware, style-tagged text)
//! that the thin ratatui drawing layer converts into `ratatui::Line`s. This
//! keeps the layout logic headless and testable.

use ratatui::style::{Color, Style};

/// A single styled span: (text, style).
pub type StyledSpan = (String, Style);

/// A rendered line.
pub type StyledLine = Vec<StyledSpan>;

pub fn plain(text: impl Into<String>) -> StyledLine {
    vec![(text.into(), Style::default())]
}

pub fn styled(text: impl Into<String>, style: Style) -> StyledLine {
    vec![(text.into(), style)]
}

pub fn empty() -> StyledLine {
    Vec::new()
}

/// Display width of a styled line.
pub fn width(line: &StyledLine) -> usize {
    line.iter()
        .map(|(s, _)| unicode_width::UnicodeWidthStr::width(s.as_str()))
        .sum()
}

/// Right-pad a line to at least `len` display cells.
pub fn pad_to(line: StyledLine, len: usize) -> StyledLine {
    let current = width(&line);
    if current >= len {
        return line;
    }
    let mut out = line;
    out.push((" ".repeat(len - current), Style::default()));
    out
}

/// Wrap plain text to `width` display cells.
pub fn wrap_plain(text: &str, width: usize) -> Vec<StyledLine> {
    crate::util::markdown::render(
        text,
        &crate::util::markdown::MarkdownOptions {
            width: width.max(8),
            conceal: true,
            fg: Color::White,
            heading: Color::White,
            code: Color::White,
            muted: Color::White,
        },
    )
    .into_iter()
    .map(|l| {
        l.spans
            .into_iter()
            .map(|s| (s.text, Style::default()))
            .collect()
    })
    .collect()
}

/// Convert to a ratatui `Line` (owned).
pub fn to_ratatui(line: &StyledLine) -> ratatui::text::Line<'static> {
    let spans: Vec<ratatui::text::Span> = line
        .iter()
        .filter(|(s, _)| !s.is_empty())
        .map(|(text, style)| ratatui::text::Span::styled(text.clone(), *style))
        .collect();
    ratatui::text::Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn width_counts_display_cells() {
        assert_eq!(width(&plain("hello")), 5);
        assert_eq!(width(&plain("日本語")), 6);
    }

    #[test]
    fn pad_to_pads() {
        let line = plain("ab");
        let padded = pad_to(line, 5);
        assert_eq!(width(&padded), 5);
    }

    #[test]
    fn styled_spans_keep_order() {
        let line: StyledLine = vec![
            ("a".to_string(), Style::default().fg(Color::Red)),
            ("b".to_string(), Style::default()),
        ];
        assert_eq!(width(&line), 2);
        let ratatui_line = to_ratatui(&line);
        assert_eq!(ratatui_line.spans.len(), 2);
    }
}
