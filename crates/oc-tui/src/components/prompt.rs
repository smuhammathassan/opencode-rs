//! Prompt textarea rendering.
//!
//! Port of the `<textarea>` + status row from
//! `reference/packages/tui/src/component/prompt/index.tsx`. The buffer text
//! contains virtual-text part markers which are styled distinctly; the cursor
//! is mapped to a visual (row, col) in display space.

use ratatui::style::{Color, Style};

use crate::components::text::{pad_to, StyledLine};
use crate::prompt::parts::{part_ranges, PartKind};
use crate::theme::Theme;

/// Wrap the display text of the buffer into visual lines and locate the cursor.
/// Returns (lines, cursor visual position, part styling per line).
pub fn layout(
    text: &str,
    width: usize,
    cursor: usize,
    parts: &[serde_json::Value],
    theme: &Theme,
) -> (Vec<StyledLine>, Option<(usize, usize)>) {
    let width = width.max(1);
    let ranges = part_ranges(parts);
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());

    let mut lines: Vec<StyledLine> = Vec::new();
    let mut current: StyledLine = Vec::new();
    let mut current_len = 0usize;
    let mut cursor_pos = None;

    let push_line = |current: &mut StyledLine, lines: &mut Vec<StyledLine>| {
        if current.is_empty() {
            lines.push(Vec::new());
        } else {
            lines.push(pad_to(std::mem::take(current), width));
        }
    };

    for (idx, &c) in chars.iter().enumerate() {
        let part_style = ranges
            .iter()
            .find(|r| r.start <= idx && idx < r.end)
            .map(|r| part_color(r.kind, theme));
        if c == '\n' {
            if idx == cursor {
                cursor_pos = Some((lines.len(), current_len));
            }
            push_line(&mut current, &mut lines);
            current_len = 0;
            continue;
        }
        let w = unicode_width::UnicodeWidthChar::width(c)
            .unwrap_or(1)
            .max(1);
        if current_len + w > width {
            push_line(&mut current, &mut lines);
            current_len = 0;
        }
        if idx == cursor {
            cursor_pos = Some((lines.len(), current_len));
        }
        let style = part_style.unwrap_or(Style::default());
        let mut text = String::new();
        text.push(c);
        if let Some((row, col)) = cursor_pos {
            let _ = (row, col);
        }
        current.push((text, style));
        current_len += w;
    }
    if cursor == chars.len() {
        cursor_pos = Some((lines.len(), current_len));
    }
    push_line(&mut current, &mut lines);
    (lines, cursor_pos)
}

fn part_color(kind: PartKind, theme: &Theme) -> Style {
    match kind {
        PartKind::File => Style::default().fg(theme.secondary),
        PartKind::Agent => Style::default().fg(theme.accent),
        PartKind::PastedText => Style::default().fg(theme.primary),
    }
}

/// Build the prompt box lines including the left border and padding.
/// `focused_border_color` is the agent/leader highlight color.
pub fn prompt_lines(
    text: &str,
    width: usize,
    cursor: usize,
    parts: &[serde_json::Value],
    theme: &Theme,
    border_color: Color,
    placeholder: Option<&str>,
) -> (Vec<StyledLine>, Option<(usize, usize)>) {
    let inner_width = width.saturating_sub(4).max(1);
    let (text_lines, cursor_pos) = layout(text, inner_width, cursor, parts, theme);
    let textarea_width = width.saturating_sub(1).max(1);

    let mut out: Vec<StyledLine> = Vec::new();
    let mut visual_cursor = None;
    let mut rendered_any = false;
    for (row, line) in text_lines.iter().enumerate() {
        let mut spans: StyledLine = vec![("┃".to_string(), Style::default().fg(border_color))];
        spans.push(("  ".to_string(), Style::default()));
        if line.is_empty() && !rendered_any && placeholder.is_some() {
            spans.push((
                placeholder.unwrap().to_string(),
                Style::default().fg(theme.text_muted),
            ));
            if cursor_pos.map(|(_, c)| c == 0).unwrap_or(true) && row == 0 {
                visual_cursor = Some((0, 3));
            }
            let span_count = spans.len();
            let _ = span_count;
        } else {
            for (text, style) in line {
                let s = *style;
                spans.push((text.clone(), s));
            }
        }
        if let Some((crow, ccol)) = cursor_pos {
            if crow == row {
                visual_cursor = Some((row, 2 + ccol));
            }
        }
        out.push(pad_to(spans, textarea_width));
        rendered_any = true;
    }

    // Bottom border row: `┃` + `▀` line.
    let mut bottom: StyledLine = vec![("┃".to_string(), Style::default().fg(border_color))];
    let fill = "▀".repeat(textarea_width.saturating_sub(1));
    bottom.push((fill, Style::default().fg(theme.background_element)));
    out.push(pad_to(bottom, textarea_width));

    (out, visual_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_wraps_and_finds_cursor() {
        let text = "hello world";
        let (lines, cursor) = layout(text, 6, 4, &[], &Theme::dark());
        assert_eq!(lines.len(), 2);
        assert_eq!(cursor, Some((0, 4)));
        let (lines, cursor) = layout(text, 6, 7, &[], &Theme::dark());
        assert_eq!(cursor, Some((1, 1)));
    }

    #[test]
    fn layout_multiline() {
        let (lines, cursor) = layout("ab\ncd", 10, 3, &[], &Theme::dark());
        assert_eq!(lines.len(), 2);
        assert_eq!(cursor, Some((1, 0)));
    }

    #[test]
    fn part_markers_are_styled() {
        let part = serde_json::json!({
            "type": "file", "mime": "image/png", "filename": "a.png", "url": "x",
            "source": { "type": "file", "path": "a.png", "text": { "value": "[Image 1]", "start": 0, "end": 9 } }
        });
        let (lines, _) = layout("[Image 1] hi", 40, 0, &[part], &Theme::dark());
        assert_eq!(lines[0][1].1.fg, Some(Color::Rgb(92, 156, 245)));
    }

    #[test]
    fn cursor_at_end() {
        let (_, cursor) = layout("hi", 10, 2, &[], &Theme::dark());
        assert_eq!(cursor, Some((0, 2)));
    }
}
