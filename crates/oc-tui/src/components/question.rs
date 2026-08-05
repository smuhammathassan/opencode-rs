//! Question prompt rendering.
//! From reference/packages/tui/src/routes/session/question.tsx

use ratatui::style::{Color, Modifier, Style};

use crate::components::text::{pad_to, StyledLine};
use crate::theme::{selected_foreground, Theme};
use crate::types::QuestionRequest;

#[derive(Debug, Clone, Default)]
pub struct QuestionState {
    pub tab: usize,
    pub answers: Vec<Vec<String>>,
    pub custom: Vec<String>,
    pub selected: usize,
    pub editing: bool,
}

impl QuestionState {
    pub fn new(question_count: usize) -> Self {
        QuestionState {
            tab: 0,
            answers: vec![Vec::new(); question_count],
            custom: vec![String::new(); question_count],
            selected: 0,
            editing: false,
        }
    }

    /// Toggle a multi-select answer for the current tab.
    pub fn toggle_answer(&mut self, answer: String) {
        let existing = &mut self.answers[self.tab];
        if let Some(idx) = existing.iter().position(|a| *a == answer) {
            existing.remove(idx);
        } else {
            existing.push(answer);
        }
    }

    pub fn set_answer(&mut self, tab: usize, answer: String) {
        if tab < self.answers.len() {
            self.answers[tab] = vec![answer];
        }
    }

    pub fn set_custom(&mut self, tab: usize, value: String) {
        if tab < self.custom.len() {
            self.custom[tab] = value;
        }
    }
}

/// Render the question prompt into lines.
pub fn render(
    request: &QuestionRequest,
    state: &QuestionState,
    width: usize,
    height: usize,
    theme: &Theme,
) -> Vec<StyledLine> {
    let questions = &request.questions;
    let single = questions.len() == 1 && questions[0].multiple != Some(true);
    let tabs = if single { 1 } else { questions.len() + 1 };
    let tab = state.tab.min(tabs.saturating_sub(1));
    let confirm = !single && tab == questions.len();
    let question = questions.get(tab);
    let mut lines: Vec<StyledLine> = Vec::new();

    // Header accent border.
    lines.push(vec![
        ("┃".to_string(), Style::default().fg(theme.accent)),
        ("  ".to_string(), Style::default()),
        ("Questions".to_string(), Style::default().fg(theme.text)),
    ]);

    if !single {
        let mut tab_line: StyledLine = vec![("  ".to_string(), Style::default())];
        for (idx, q) in questions.iter().enumerate() {
            let active = idx == tab;
            let answered = !state.answers[idx].is_empty();
            tab_line.push((
                format!(" {} ", q.header),
                Style::default()
                    .fg(if active {
                        selected_foreground(theme.accent)
                    } else if answered {
                        theme.text
                    } else {
                        theme.text_muted
                    })
                    .bg(if active {
                        theme.accent
                    } else {
                        theme.background_element
                    }),
            ));
            tab_line.push(("  ".to_string(), Style::default()));
        }
        tab_line.push((
            " Confirm ".to_string(),
            Style::default()
                .fg(if confirm {
                    selected_foreground(theme.accent)
                } else {
                    theme.text_muted
                })
                .bg(if confirm {
                    theme.accent
                } else {
                    theme.background_element
                }),
        ));
        lines.push(pad_to(tab_line, width));
    }

    if !confirm {
        if let Some(question) = question {
            let multi = question.multiple == Some(true);
            lines.push(vec![
                ("    ".to_string(), Style::default()),
                (
                    format!(
                        "{}{}",
                        question.question,
                        if multi {
                            " (select all that apply)"
                        } else {
                            ""
                        }
                    ),
                    Style::default().fg(theme.text),
                ),
            ]);
            let mut selected = state.selected;
            let total = question.options.len() + if question.custom != Some(false) { 1 } else { 0 };
            if total > 0 {
                selected = selected.min(total - 1);
            }
            for (idx, option) in question.options.iter().enumerate() {
                let active = idx == selected;
                let picked = state.answers[tab].iter().any(|a| a == &option.label);
                let bg = if active {
                    theme.background_element
                } else {
                    theme.background_panel
                };
                let mut spans: StyledLine = vec![("      ".to_string(), Style::default())];
                spans.push((
                    format!("{}.", idx + 1),
                    Style::default()
                        .fg(if active {
                            theme.secondary
                        } else {
                            theme.text_muted
                        })
                        .bg(bg),
                ));
                spans.push((" ".to_string(), Style::default().bg(bg)));
                let label = if multi {
                    format!("[{}] {}", if picked { "✓" } else { " " }, option.label)
                } else {
                    option.label.clone()
                };
                spans.push((
                    label,
                    Style::default()
                        .fg(if active {
                            theme.secondary
                        } else if picked {
                            theme.success
                        } else {
                            theme.text
                        })
                        .bg(bg),
                ));
                if !multi && picked {
                    spans.push((" ✓".to_string(), Style::default().fg(theme.success).bg(bg)));
                }
                lines.push(pad_to(spans, width));
                if !option.description.is_empty() {
                    lines.push(vec![
                        ("            ".to_string(), Style::default()),
                        (
                            option.description.clone(),
                            Style::default().fg(theme.text_muted),
                        ),
                    ]);
                }
            }
            if question.custom != Some(false) {
                let custom_idx = question.options.len();
                let other = selected == custom_idx;
                let custom_value = state.custom.get(tab).cloned().unwrap_or_default();
                let custom_picked = !custom_value.is_empty()
                    && state.answers[tab].iter().any(|a| *a == custom_value);
                let bg = if other {
                    theme.background_element
                } else {
                    theme.background_panel
                };
                let mut spans: StyledLine = vec![("      ".to_string(), Style::default())];
                spans.push((
                    format!("{}.", custom_idx + 1),
                    Style::default()
                        .fg(if other {
                            theme.secondary
                        } else {
                            theme.text_muted
                        })
                        .bg(bg),
                ));
                spans.push((" ".to_string(), Style::default().bg(bg)));
                let label = if multi {
                    format!(
                        "[{}] Type your own answer",
                        if custom_picked { "✓" } else { " " }
                    )
                } else {
                    "Type your own answer".to_string()
                };
                spans.push((
                    label,
                    Style::default()
                        .fg(if other {
                            theme.secondary
                        } else if custom_picked {
                            theme.success
                        } else {
                            theme.text
                        })
                        .bg(bg),
                ));
                if !multi && custom_picked {
                    spans.push((" ✓".to_string(), Style::default().fg(theme.success).bg(bg)));
                }
                lines.push(pad_to(spans, width));
                if state.editing && other {
                    let value = if custom_value.is_empty() {
                        "Type your own answer".to_string()
                    } else {
                        custom_value.clone()
                    };
                    lines.push(vec![
                        ("            ".to_string(), Style::default()),
                        (
                            value,
                            Style::default().fg(theme.text).bg(theme.background_element),
                        ),
                    ]);
                } else if !state.editing && !custom_value.is_empty() {
                    lines.push(vec![
                        ("            ".to_string(), Style::default()),
                        (custom_value.clone(), Style::default().fg(theme.text_muted)),
                    ]);
                }
            }
        }
    } else {
        // Review tab.
        lines.push(vec![
            ("    ".to_string(), Style::default()),
            ("Review".to_string(), Style::default().fg(theme.text)),
        ]);
        for (idx, q) in questions.iter().enumerate() {
            let value = state.answers[idx].join(", ");
            let answered = !value.is_empty();
            lines.push(vec![
                ("    ".to_string(), Style::default()),
                (
                    format!("{}: ", q.header),
                    Style::default().fg(theme.text_muted),
                ),
                (
                    if answered {
                        value
                    } else {
                        "(not answered)".to_string()
                    },
                    Style::default().fg(if answered { theme.text } else { theme.error }),
                ),
            ]);
        }
    }

    // Footer hints.
    let mut footer: StyledLine = vec![("  ".to_string(), Style::default())];
    if !single {
        footer.push(("⇆ tab  ".to_string(), Style::default().fg(theme.text)));
        footer.push(("↑↓ select  ".to_string(), Style::default().fg(theme.text)));
    }
    let action = if confirm {
        "submit"
    } else if question.map(|q| q.multiple == Some(true)).unwrap_or(false) {
        "toggle"
    } else if single {
        "submit"
    } else {
        "confirm"
    };
    footer.push(("enter ".to_string(), Style::default().fg(theme.text)));
    footer.push((action.to_string(), Style::default().fg(theme.text_muted)));
    footer.push(("  esc dismiss".to_string(), Style::default().fg(theme.text)));
    lines.push(pad_to(footer, width));
    let _ = (height, Modifier::empty());
    let _ = Color::White;
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> QuestionRequest {
        serde_json::from_value(json!({
            "id": "que_1", "sessionID": "ses_1",
            "questions": [
                { "question": "Pick a language?", "header": "Lang", "options": [
                    { "label": "Rust", "description": "safe" },
                    { "label": "Go", "description": "simple" }
                ] },
                { "question": "Confirm?", "header": "Confirm", "options": [] }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn renders_options_with_numbers() {
        let request = request();
        let state = QuestionState::new(2);
        let lines = render(&request, &state, 60, 20, &Theme::dark());
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Pick a language"))));
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("1."))));
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Rust"))));
    }

    #[test]
    fn tabs_rendered_for_multiple() {
        let request = request();
        let state = QuestionState::new(2);
        let lines = render(&request, &state, 60, 20, &Theme::dark());
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Lang"))));
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Confirm"))));
    }

    #[test]
    fn review_shows_unanswered() {
        let request = request();
        let mut state = QuestionState::new(2);
        state.tab = 2; // confirm tab
        let lines = render(&request, &state, 60, 20, &Theme::dark());
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("Review"))));
        assert!(lines
            .iter()
            .any(|l| l.iter().any(|(s, _)| s.contains("(not answered)"))));
    }
}
