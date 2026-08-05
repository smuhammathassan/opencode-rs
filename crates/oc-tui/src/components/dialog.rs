//! Dialog list rendering and navigation.
//!
//! Ports the shared dialog/select behavior from
//! `reference/packages/tui/src/ui/dialog-select.tsx` and the dialogs in
//! `component/dialog-*.tsx`. The app owns dialog state; this module provides
//! the pure filtering/navigation and line rendering.

use ratatui::style::{Color, Modifier, Style};

use crate::components::text::{pad_to, StyledLine};
use crate::prompt::autocomplete::fuzzy_score;
use crate::theme::{selected_foreground, Theme};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogItem {
    pub title: String,
    pub description: Option<String>,
    /// Marks the currently-selected option (e.g. the active model/agent).
    pub selected: bool,
}

impl DialogItem {
    pub fn new(title: impl Into<String>) -> Self {
        DialogItem {
            title: title.into(),
            description: None,
            selected: false,
        }
    }
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Filter item indices by the query using fuzzy relevance.
pub fn filter_items<'a>(items: &'a [DialogItem], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let mut scored: Vec<(f32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            fuzzy_score(query, &item.title).map(|score| {
                let bonus = if item.title.to_lowercase().starts_with(&query.to_lowercase()) {
                    1.0
                } else {
                    0.0
                };
                (score + bonus, idx)
            })
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, idx)| idx).collect()
}

/// Move the dialog selection by `delta` within `total` items.
pub fn move_selection(selected: usize, total: usize, delta: i32) -> usize {
    if total == 0 {
        return 0;
    }
    (selected as i32 + delta).rem_euclid(total as i32) as usize
}

/// Render a bordered list dialog into lines. `visible` maps the selected item
/// to its index within `filtered`.
pub fn render_list(
    title: &str,
    items: &[DialogItem],
    filtered: &[usize],
    selected: usize,
    width: usize,
    theme: &Theme,
    height: usize,
) -> (Vec<StyledLine>, usize) {
    let max_visible = height.saturating_sub(3).max(1);
    let visible = filtered.len().min(max_visible);
    let selected = selected.min(filtered.len().saturating_sub(1));
    let start = if filtered.len() <= max_visible {
        0
    } else {
        selected
    };
    let selected_visible = selected.saturating_sub(start);

    let mut lines: Vec<StyledLine> = Vec::new();
    let mut header: StyledLine = vec![
        ("┏ ".to_string(), Style::default().fg(theme.border_active)),
        (
            title.to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
    ];
    header.push((
        " ━".repeat(width.saturating_sub(header_len(&header))),
        Style::default().fg(theme.border),
    ));
    lines.push(pad_to(header, width));

    for offset in 0..visible {
        let item_index = filtered.get(start + offset).copied().unwrap_or(0);
        let item = &items[item_index];
        let active = offset == selected_visible;
        let bg = if active {
            theme.primary
        } else {
            theme.background_panel
        };
        let fg = if active {
            selected_foreground(theme.primary)
        } else if item.selected {
            theme.text
        } else {
            theme.text
        };
        let mut spans: StyledLine = Vec::new();
        spans.push((" ".to_string(), Style::default().bg(bg)));
        let marker = if item.selected { "● " } else { "  " };
        spans.push((
            marker.to_string(),
            Style::default().fg(fg).bg(bg).add_modifier(if active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ));
        spans.push((item.title.clone(), Style::default().fg(fg).bg(bg)));
        if let Some(description) = &item.description {
            spans.push((" ".to_string(), Style::default().bg(bg)));
            spans.push((
                description.clone(),
                Style::default().fg(theme.text_muted).bg(bg),
            ));
        }
        lines.push(pad_to(spans, width));
    }
    for _ in lines.len()..max_visible + 1 {
        lines.push(pad_to(StyledLine::new(), width));
    }
    let mut footer: StyledLine = vec![
        ("└ ".to_string(), Style::default().fg(theme.border_active)),
        (
            format!("{} items", filtered.len()),
            Style::default().fg(theme.text_muted),
        ),
    ];
    lines.push(pad_to(footer, width));

    (lines, selected_visible)
}

fn header_len(line: &StyledLine) -> usize {
    line.iter()
        .map(|(s, _)| unicode_width::UnicodeWidthStr::width(s.as_str()))
        .sum()
}

/// Render a confirm/alert dialog into lines.
pub fn render_confirm(
    title: &str,
    message: &str,
    options: &[String],
    selected: usize,
    width: usize,
    theme: &Theme,
) -> Vec<StyledLine> {
    let mut lines: Vec<StyledLine> = Vec::new();
    lines.push(pad_to(
        vec![(
            format!("┏ {title}"),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )],
        width,
    ));
    let message_width = width.saturating_sub(4).max(10);
    for line in wrap(message, message_width) {
        let mut spans = vec![("┃  ".to_string(), Style::default().fg(theme.border_active))];
        spans.extend(line);
        lines.push(pad_to(spans, width));
    }
    lines.push(vec![(
        "┃".to_string(),
        Style::default().fg(theme.border_active),
    )]);
    let mut option_line: StyledLine = Vec::new();
    for (idx, option) in options.iter().enumerate() {
        let active = idx == selected;
        let bg = if active {
            theme.primary
        } else {
            theme.background_element
        };
        let fg = if active {
            selected_foreground(theme.primary)
        } else {
            theme.text_muted
        };
        option_line.push((
            format!(" {option} "),
            Style::default().fg(fg).bg(bg).add_modifier(if active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ));
    }
    option_line.push(("  ".to_string(), Style::default()));
    lines.push(vec![
        ("┃ ".to_string(), Style::default().fg(theme.border_active)),
        ("".to_string(), Style::default()),
    ]);
    lines.push(pad_to(
        {
            let mut l: StyledLine =
                vec![("┃  ".to_string(), Style::default().fg(theme.border_active))];
            l.extend(option_line);
            l
        },
        width,
    ));
    lines.push(pad_to(
        vec![("┗".to_string(), Style::default().fg(theme.border_active))],
        width,
    ));
    lines
}

fn wrap(text: &str, width: usize) -> Vec<StyledLine> {
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

/// Static help dialog lines. Mirrors the keybind listing of
/// reference/packages/tui/src/ui/dialog-help.tsx.
pub fn help_lines(
    width: usize,
    theme: &Theme,
    keybinds: &crate::config::ResolvedConfig,
) -> Vec<StyledLine> {
    use crate::keybind::definitions;
    let mut lines: Vec<StyledLine> = Vec::new();
    lines.push(pad_to(
        vec![(
            " OpenCode Help ".to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )],
        width,
    ));
    lines.push(vec![("".to_string(), Style::default().fg(theme.border))]);
    let bindings: Vec<&crate::keybind::KeybindDef> = definitions()
        .iter()
        .filter(|d| !d.command.is_empty() && d.default != "none")
        .filter(|d| {
            matches!(
                d.command,
                "model.list"
                    | "agent.list"
                    | "command.palette.show"
                    | "session.list"
                    | "session.new"
                    | "session.interrupt"
                    | "session.undo"
                    | "session.rename"
                    | "prompt.editor"
                    | "messages.copy"
                    | "session.toggle.conceal"
                    | "session.toggle.actions"
                    | "session.toggle.thinking"
                    | "session.sidebar.toggle"
                    | "app.exit"
            )
        })
        .collect();
    for binding in bindings {
        let shortcut = keybinds
            .get(binding.name)
            .map(|b| {
                b.sequences
                    .first()
                    .map(|seq| {
                        seq.strokes
                            .iter()
                            .map(|s| match s {
                                crate::keymap::Stroke::Leader => crate::keymap::leader_key_name(),
                                crate::keymap::Stroke::Key(k) => k.display(),
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let mut spans: StyledLine = vec![
            ("  ".to_string(), Style::default()),
            (format!("{shortcut:<12}"), Style::default().fg(theme.text)),
            (
                binding.desc.to_string(),
                Style::default().fg(theme.text_muted),
            ),
        ];
        spans.push(("  ".to_string(), Style::default()));
        lines.push(pad_to(spans, width));
    }
    lines.push(vec![("".to_string(), Style::default().fg(theme.border))]);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<DialogItem> {
        vec![
            DialogItem::new("model"),
            DialogItem::new("models"),
            DialogItem::new("move"),
        ]
    }

    #[test]
    fn empty_filter_returns_all() {
        assert_eq!(filter_items(&items(), ""), vec![0, 1, 2]);
    }

    #[test]
    fn filter_ranks_relevant() {
        let filtered = filter_items(&items(), "model");
        assert_eq!(filtered[0], 0);
        assert_eq!(filtered[1], 1);
    }

    #[test]
    fn selection_wraps() {
        assert_eq!(move_selection(0, 3, -1), 2);
        assert_eq!(move_selection(2, 3, 1), 0);
        assert_eq!(move_selection(1, 0, 1), 0);
    }

    #[test]
    fn list_render_marks_selected() {
        let items = items();
        let filtered = filter_items(&items, "");
        let (lines, selected_visible) =
            render_list("Test", &items, &filtered, 1, 40, &Theme::dark(), 10);
        assert_eq!(selected_visible, 1);
        assert!(lines[0].iter().any(|(s, _)| s.contains("Test")));
        assert!(lines[2].iter().any(|(s, _)| s.contains("models")));
    }

    #[test]
    fn confirm_renders_options() {
        let lines = render_confirm(
            "Confirm",
            "Are you sure?",
            &["Yes".to_string(), "No".to_string()],
            0,
            60,
            &Theme::dark(),
        );
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Yes"))));
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("No"))));
    }

    #[test]
    fn help_lists_bindings() {
        let lines = help_lines(
            80,
            &Theme::dark(),
            &crate::config::ResolvedConfig::default_config(),
        );
        assert!(lines.len() > 5);
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("ctrl+p"))));
    }
}
