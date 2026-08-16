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

/// The kind of dialog currently open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKind {
    ModelList,
    AgentList,
    SkillList,
    BackgroundJobs,
    SessionList,
    ProviderList,
    CommandPalette,
    StashList,
    Help,
    Rename,
    Confirm {
        title: String,
        message: String,
    },
    Alert {
        title: String,
        message: String,
    },
    InfoItems {
        title: String,
        items: Vec<DialogItem>,
    },
}

/// Dialog interaction state (selection + filter).
#[derive(Debug, Clone)]
pub struct DialogState {
    pub kind: DialogKind,
    pub selected: usize,
    pub filter: String,
}

impl DialogState {
    pub fn new(kind: DialogKind) -> Self {
        DialogState {
            kind,
            selected: 0,
            filter: String::new(),
        }
    }
}

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
///
/// Search both the visible label and its description. Session switchers put
/// the session id in the description, while model switchers put the provider
/// id there, so both remain discoverable without changing their compact rows.
pub fn filter_items(items: &[DialogItem], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let query = query.trim();
    let mut scored: Vec<(f32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let searchable = match &item.description {
                Some(description) if !description.is_empty() => {
                    format!("{} {}", item.title, description)
                }
                _ => item.title.clone(),
            };
            fuzzy_score(query, &searchable).map(|score| {
                let query = query.to_lowercase();
                let title = item.title.to_lowercase();
                let description = item
                    .description
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase();
                let bonus = if title.starts_with(&query) || description.starts_with(&query) {
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
    render_list_filtered(title, items, filtered, selected, width, theme, height, "")
}

/// Render a list dialog with a visible filter prompt.
pub fn render_list_filtered(
    title: &str,
    items: &[DialogItem],
    filtered: &[usize],
    selected: usize,
    width: usize,
    theme: &Theme,
    height: usize,
    filter: &str,
) -> (Vec<StyledLine>, usize) {
    let height = height.max(3);
    let content_height = height.saturating_sub(2).max(1);
    let selected = selected.min(filtered.len().saturating_sub(1));

    let mut rows: Vec<(usize, String, bool)> = Vec::new();
    for (filtered_index, item_index) in filtered.iter().copied().enumerate() {
        let item = &items[item_index];
        let text = match &item.description {
            Some(description) if !description.is_empty() => {
                format!("{} — {}", item.title, description)
            }
            _ => item.title.clone(),
        };
        let body_width = width.saturating_sub(3).max(1);
        let wrapped = wrap_display(&text, body_width);
        for (line_index, line) in wrapped.into_iter().enumerate() {
            rows.push((filtered_index, line, line_index == 0));
        }
    }

    if rows.is_empty() {
        rows.push((usize::MAX, "No matches".to_string(), true));
    }

    let selected_row = rows
        .iter()
        .position(|(filtered_index, _, first)| *first && *filtered_index == selected)
        .unwrap_or(0);
    let start = if rows.len() <= content_height {
        0
    } else if selected_row < content_height {
        0
    } else {
        (selected_row + 1).saturating_sub(content_height)
    };
    let selected_visible = selected_row.saturating_sub(start);

    let mut lines: Vec<StyledLine> = Vec::new();
    let header_title = if filter.is_empty() {
        title.to_string()
    } else {
        format!("{title} /{filter}")
    };
    let header_title = truncate_display(&header_title, width.saturating_sub(2));
    let mut header: StyledLine = vec![
        ("┏ ".to_string(), Style::default().fg(theme.border_active)),
        (
            header_title,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
    ];
    header.push((
        "━".repeat(width.saturating_sub(header_len(&header))),
        Style::default().fg(theme.border),
    ));
    lines.push(pad_to(header, width));

    for offset in 0..content_height {
        let row_index = start + offset;
        let Some((filtered_index, text, first_line)) = rows.get(row_index) else {
            lines.push(pad_to(StyledLine::new(), width));
            continue;
        };
        let active = *filtered_index == selected && *filtered_index != usize::MAX;
        let bg = if active {
            theme.primary
        } else {
            theme.background_panel
        };
        let fg = if active || *filtered_index == usize::MAX {
            selected_foreground(theme.primary)
        } else {
            theme.text
        };
        let mut spans: StyledLine = Vec::new();
        spans.push((" ".to_string(), Style::default().bg(bg)));
        let marker = if *first_line {
            filtered
                .get(*filtered_index)
                .and_then(|item_index| items.get(*item_index))
                .map(|item| if item.selected { "● " } else { "  " })
                .unwrap_or("  ")
        } else {
            "  "
        };
        spans.push((
            marker.to_string(),
            Style::default().fg(fg).bg(bg).add_modifier(if active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ));
        spans.push((text.clone(), Style::default().fg(fg).bg(bg)));
        lines.push(pad_to(spans, width));
    }
    let footer: StyledLine = vec![
        ("└ ".to_string(), Style::default().fg(theme.border_active)),
        (
            truncate_display(
                &format!("{} items", filtered.len()),
                width.saturating_sub(2),
            ),
            Style::default().fg(theme.text_muted),
        ),
    ];
    lines.push(pad_to(footer, width));

    (lines, selected_visible)
}

/// Wrap a dialog row by display cells, breaking long ids and model names too.
fn wrap_display(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = unicode_width::UnicodeWidthStr::width(word);
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            let mut chunk = String::new();
            let mut chunk_width = 0;
            for character in word.chars() {
                let character_width =
                    unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
                if chunk_width + character_width > width && !chunk.is_empty() {
                    lines.push(std::mem::take(&mut chunk));
                    chunk_width = 0;
                }
                chunk.push(character);
                chunk_width += character_width;
            }
            if !chunk.is_empty() {
                current = chunk;
                current_width = chunk_width;
            }
            continue;
        }

        let separator = usize::from(!current.is_empty());
        if current_width + separator + word_width > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if !current.is_empty() {
            current.push(' ');
            current_width += 1;
        }
        current.push_str(word);
        current_width += word_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn truncate_display(text: &str, width: usize) -> String {
    if unicode_width::UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result
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
    fn filter_searches_descriptions() {
        let items = vec![
            DialogItem::new("Fix auth").with_description("ses_old"),
            DialogItem::new("Fix tests").with_description("ses_new"),
        ];
        assert_eq!(filter_items(&items, "ses_new"), vec![1]);
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
    fn list_render_wraps_long_switcher_rows() {
        let items =
            vec![DialogItem::new("provider-with-a-very-long-model-name")
                .with_description("provider-id")];
        let filtered = filter_items(&items, "");
        let (lines, _) =
            render_list_filtered("Models", &items, &filtered, 0, 20, &Theme::dark(), 8, "");
        assert!(lines
            .iter()
            .all(|line| crate::components::text::width(line) <= 20));
        assert!(lines.iter().filter(|line| !line.is_empty()).count() > 3);
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
