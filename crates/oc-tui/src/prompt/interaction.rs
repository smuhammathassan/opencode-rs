//! Prompt input interaction state machine.
//!
//! Faithful port of `reference/packages/session-ui/src/v2/components/prompt-input/machine.ts`
//! (`createPromptInputV2InteractionState`, `transitionPromptInputV2`). Serialization
//! shapes mirror the reference's JSON structure (tagged by `type`, camelCase fields,
//! optional fields omitted when absent) so differential comparisons are canonical.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionState {
    pub mode: Mode,
    pub popover: Popover,
    pub drag: Drag,
    pub focus: Focus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeContextID"
    )]
    pub active_context_id: Option<String>,
    pub history_index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_history: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Normal,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Popover {
    Closed,
    Context {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "activeID")]
        active_id: Option<String>,
    },
    CommandInline {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "activeID")]
        active_id: Option<String>,
    },
    CommandMenu {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "activeID")]
        active_id: Option<String>,
    },
}

impl Popover {
    fn kind(&self) -> &'static str {
        match self {
            Popover::Closed => "closed",
            Popover::Context { .. } => "context",
            Popover::CommandInline { .. } | Popover::CommandMenu { .. } => "command",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Drag {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Focus {
    Editor,
    #[serde(rename = "command-search")]
    CommandSearch,
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Event {
    #[serde(rename_all = "camelCase")]
    InputChanged {
        value: String,
        #[serde(default)]
        persist: Option<bool>,
    },
    CommandsOpen,
    ContextOpen,
    #[serde(rename_all = "camelCase")]
    PopoverQuery {
        value: String,
    },
    #[serde(rename_all = "camelCase")]
    PopoverResults {
        ids: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    PopoverActive {
        id: String,
    },
    PopoverClose,
    #[serde(rename_all = "camelCase")]
    PopoverSelect {
        item: Suggestion,
    },
    KeyDown {
        key: String,
        ctrl: bool,
        composing: bool,
        ids: Vec<String>,
        #[serde(default)]
        empty: Option<bool>,
    },
    #[serde(rename = "mode.shell")]
    ModeShell,
    #[serde(rename = "mode.normal")]
    ModeNormal,
    #[serde(rename = "drag.enter")]
    DragEnter,
    #[serde(rename = "drag.leave")]
    DragLeave,
    #[serde(rename = "focus.editor")]
    FocusEditor,
    #[serde(rename = "focus.external")]
    FocusExternal,
    #[serde(rename = "context.active")]
    ContextActive {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Command {
    #[serde(rename = "draft.setText", rename_all = "camelCase")]
    DraftSetText { value: String },
    #[serde(rename = "mention.add", rename_all = "camelCase")]
    MentionAdd { item: Suggestion },
    #[serde(rename = "popover.filter", rename_all = "camelCase")]
    PopoverFilter { popover: String, query: String },
    #[serde(rename = "suggestion.select", rename_all = "camelCase")]
    SuggestionSelect { id: String },
    #[serde(rename = "focus.editor")]
    FocusEditor,
    #[serde(rename = "focus.command-search")]
    FocusCommandSearch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub state: InteractionState,
    pub commands: Vec<Command>,
    pub handled: bool,
}

/// From reference `PromptInputV2PersistedState` (subset used by transitions).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    #[serde(default)]
    pub prompt: Vec<PromptPart>,
    #[serde(default)]
    pub context: ContextState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PromptPart {
    Text {
        content: String,
    },
    File {
        #[serde(default)]
        path: Option<String>,
    },
    Image {
        #[serde(default)]
        path: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextState {
    #[serde(default)]
    pub items: Vec<serde_json::Value>,
}

pub fn create_interaction_state() -> InteractionState {
    InteractionState {
        mode: Mode::Normal,
        popover: Popover::Closed,
        drag: Drag::Idle,
        focus: Focus::External,
        active_context_id: None,
        history_index: -1,
        saved_history: None,
    }
}

/// From reference `transitionPromptInputV2`.
pub fn transition(
    state: &InteractionState,
    event: &Event,
    persisted: &PersistedState,
) -> Transition {
    match event {
        Event::InputChanged { value, persist } => {
            input_changed(state, value, *persist != Some(false), persisted.cursor)
        }
        Event::CommandsOpen => open_commands(state, persisted),
        Event::ContextOpen => open_context(state, persisted),
        Event::PopoverQuery { value } => query_changed(state, value),
        Event::PopoverResults { ids } => results_changed(state, ids),
        Event::PopoverActive { id } => active_changed(state, id),
        Event::PopoverClose => changed(
            &mut clone_with(state, |s| s.popover = Popover::Closed),
            vec![],
            false,
        ),
        Event::PopoverSelect { item } => suggestion_selected(state, item, persisted),
        Event::KeyDown {
            key,
            ctrl,
            composing,
            ids,
            empty,
        } => key_down(state, key, *ctrl, *composing, ids, empty.unwrap_or(false)),
        Event::ModeShell => changed(
            &mut clone_with(state, |s| {
                s.mode = Mode::Shell;
                s.popover = Popover::Closed;
            }),
            vec![],
            false,
        ),
        Event::ModeNormal => changed(
            &mut clone_with(state, |s| s.mode = Mode::Normal),
            vec![],
            false,
        ),
        Event::DragEnter => changed(
            &mut clone_with(state, |s| s.drag = Drag::Active),
            vec![],
            false,
        ),
        Event::DragLeave => changed(
            &mut clone_with(state, |s| s.drag = Drag::Idle),
            vec![],
            false,
        ),
        Event::FocusEditor => changed(
            &mut clone_with(state, |s| s.focus = Focus::Editor),
            vec![],
            false,
        ),
        Event::ContextActive { id } => changed(
            &mut clone_with(state, |s| {
                s.active_context_id = if s.active_context_id.as_deref() == Some(id.as_str()) {
                    None
                } else {
                    Some(id.clone())
                }
            }),
            vec![],
            false,
        ),
        Event::FocusExternal => changed(
            &mut clone_with(state, |s| s.focus = Focus::External),
            vec![],
            false,
        ),
    }
}

fn clone_with(state: &InteractionState, f: impl FnOnce(&mut InteractionState)) -> InteractionState {
    let mut s = state.clone();
    f(&mut s);
    s
}

fn input_changed(
    state: &InteractionState,
    value: &str,
    persist: bool,
    cursor: Option<usize>,
) -> Transition {
    let set_text: Vec<Command> = if persist {
        vec![Command::DraftSetText {
            value: value.to_string(),
        }]
    } else {
        vec![]
    };
    if state.mode == Mode::Normal && value == "!" {
        return changed(
            &mut clone_with(state, |s| {
                s.mode = Mode::Shell;
                s.popover = Popover::Closed;
                s.focus = Focus::Editor;
            }),
            vec![Command::DraftSetText {
                value: String::new(),
            }],
            false,
        );
    }
    let scoped: Cow<'_, str> = match cursor {
        Some(c) => Cow::Owned(value.chars().take(c).collect::<String>()),
        None => Cow::Borrowed(value),
    };
    if let Some(query) = trailing_context_query(&scoped) {
        let mut commands = set_text;
        commands.push(Command::PopoverFilter {
            popover: "context".to_string(),
            query: query.clone(),
        });
        return changed(
            &mut clone_with(state, |s| {
                s.popover = Popover::Context {
                    query,
                    active_id: None,
                };
                s.focus = Focus::Editor;
            }),
            commands,
            false,
        );
    }
    if let Some(query) = leading_command_query(value) {
        let mut commands = set_text;
        commands.push(Command::PopoverFilter {
            popover: "command".to_string(),
            query: query.clone(),
        });
        return changed(
            &mut clone_with(state, |s| {
                s.popover = Popover::CommandInline {
                    query,
                    active_id: None,
                };
                s.focus = Focus::Editor;
            }),
            commands,
            false,
        );
    }
    let mut next = state.clone();
    next.popover = match &state.popover {
        p @ Popover::CommandMenu { .. } => p.clone(),
        _ => Popover::Closed,
    };
    next.focus = Focus::Editor;
    changed(&mut next, set_text, false)
}

/// Mirrors reference regex `(?:^|\s)@([^\s@]*)$` on the scoped text.
fn trailing_context_query(scoped: &str) -> Option<String> {
    let at = scoped.rfind('@')?;
    let preceded_by_boundary = match scoped[..at].chars().next_back() {
        Some(c) => c.is_whitespace(),
        None => true,
    };
    if !preceded_by_boundary {
        return None;
    }
    let token = &scoped[at + 1..];
    if token.chars().any(|c| c.is_whitespace() || c == '@') {
        return None;
    }
    Some(token.to_string())
}

/// Mirrors reference regex `^\/(\S*)$` on the full value.
fn leading_command_query(value: &str) -> Option<String> {
    let rest = value.strip_prefix('/')?;
    if rest.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    Some(rest.to_string())
}

fn open_commands(state: &InteractionState, persisted: &PersistedState) -> Transition {
    if !populated(persisted) {
        return changed(
            &mut clone_with(state, |s| {
                s.popover = Popover::CommandInline {
                    query: String::new(),
                    active_id: None,
                };
                s.focus = Focus::Editor;
            }),
            vec![
                Command::DraftSetText {
                    value: format!("{}/", prompt_text(persisted)),
                },
                Command::PopoverFilter {
                    popover: "command".to_string(),
                    query: String::new(),
                },
                Command::FocusEditor,
            ],
            false,
        );
    }
    changed(
        &mut clone_with(state, |s| {
            s.popover = Popover::CommandMenu {
                query: String::new(),
                active_id: None,
            };
            s.focus = Focus::CommandSearch;
        }),
        vec![
            Command::PopoverFilter {
                popover: "command".to_string(),
                query: String::new(),
            },
            Command::FocusCommandSearch,
        ],
        false,
    )
}

fn open_context(state: &InteractionState, persisted: &PersistedState) -> Transition {
    changed(
        &mut clone_with(state, |s| {
            s.popover = Popover::Context {
                query: String::new(),
                active_id: None,
            };
            s.focus = Focus::Editor;
        }),
        vec![
            Command::DraftSetText {
                value: format!("{}@", prompt_text(persisted)),
            },
            Command::PopoverFilter {
                popover: "context".to_string(),
                query: String::new(),
            },
            Command::FocusEditor,
        ],
        false,
    )
}

fn query_changed(state: &InteractionState, query: &str) -> Transition {
    if state.popover == Popover::Closed {
        return unchanged(state, false);
    }
    let popover_kind = state.popover.kind();
    changed(
        &mut clone_with(state, |s| {
            s.popover = match &state.popover {
                Popover::Context { active_id, .. } => Popover::Context {
                    query: query.to_string(),
                    active_id: active_id.clone(),
                },
                Popover::CommandInline { active_id, .. } => Popover::CommandInline {
                    query: query.to_string(),
                    active_id: active_id.clone(),
                },
                Popover::CommandMenu { active_id, .. } => Popover::CommandMenu {
                    query: query.to_string(),
                    active_id: active_id.clone(),
                },
                Popover::Closed => Popover::Closed,
            };
        }),
        vec![Command::PopoverFilter {
            popover: popover_kind.to_string(),
            query: query.to_string(),
        }],
        false,
    )
}

fn results_changed(state: &InteractionState, ids: &[String]) -> Transition {
    if state.popover == Popover::Closed {
        return unchanged(state, false);
    }
    let current_active = match &state.popover {
        Popover::Context { active_id, .. }
        | Popover::CommandInline { active_id, .. }
        | Popover::CommandMenu { active_id, .. } => active_id.clone(),
        Popover::Closed => None,
    };
    let active_id = match &current_active {
        Some(id) if ids.contains(id) => Some(id.clone()),
        _ => ids.first().cloned(),
    };
    if active_id == current_active {
        return unchanged(state, false);
    }
    changed(
        &mut clone_with(state, |s| set_popover_active(&mut s.popover, active_id)),
        vec![],
        false,
    )
}

fn active_changed(state: &InteractionState, id: &str) -> Transition {
    if state.popover == Popover::Closed {
        return unchanged(state, false);
    }
    if let Popover::Context { active_id, .. }
    | Popover::CommandInline { active_id, .. }
    | Popover::CommandMenu { active_id, .. } = &state.popover
    {
        if active_id.as_deref() == Some(id) {
            return unchanged(state, false);
        }
    }
    changed(
        &mut clone_with(state, |s| {
            set_popover_active(&mut s.popover, Some(id.to_string()))
        }),
        vec![],
        false,
    )
}

fn set_popover_active(popover: &mut Popover, active_id: Option<String>) {
    match popover {
        Popover::Context { active_id: a, .. }
        | Popover::CommandInline { active_id: a, .. }
        | Popover::CommandMenu { active_id: a, .. } => *a = active_id,
        Popover::Closed => {}
    }
}

fn suggestion_selected(
    state: &InteractionState,
    item: &Suggestion,
    persisted: &PersistedState,
) -> Transition {
    let current = prompt_text(persisted);
    let mut commands: Vec<Command> = Vec::new();
    if item.kind == "command" {
        let value = match &state.popover {
            Popover::CommandMenu { .. } => {
                let trimmed = current.trim();
                if trimmed.is_empty() {
                    format!("{} ", item.label)
                } else {
                    format!("{} {}", item.label, trimmed)
                }
            }
            _ => replace_trigger(&current, '/', &format!("{} ", item.label)),
        };
        commands.push(Command::DraftSetText { value });
    } else {
        commands.push(Command::MentionAdd { item: item.clone() });
    }
    commands.push(Command::FocusEditor);
    changed(
        &mut clone_with(state, |s| {
            s.popover = Popover::Closed;
            s.focus = Focus::Editor;
        }),
        commands,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn key_down(
    state: &InteractionState,
    key: &str,
    ctrl: bool,
    composing: bool,
    ids: &[String],
    empty: bool,
) -> Transition {
    if ctrl && key.eq_ignore_ascii_case("g") {
        if state.popover == Popover::Closed {
            return unchanged(state, false);
        }
        return changed(
            &mut clone_with(state, |s| {
                s.popover = Popover::Closed;
                s.focus = Focus::Editor;
            }),
            vec![Command::FocusEditor],
            true,
        );
    }
    if state.popover == Popover::Closed {
        if state.mode == Mode::Shell && (key == "Escape" || (key == "Backspace" && empty)) {
            return changed(
                &mut clone_with(state, |s| s.mode = Mode::Normal),
                vec![],
                true,
            );
        }
        return unchanged(state, false);
    }
    if key == "Escape" {
        return changed(
            &mut clone_with(state, |s| {
                s.popover = Popover::Closed;
                s.focus = Focus::Editor;
            }),
            vec![Command::FocusEditor],
            true,
        );
    }
    if key == "Tab" || (key == "Enter" && !composing) {
        let active_id = match &state.popover {
            Popover::Context { active_id, .. }
            | Popover::CommandInline { active_id, .. }
            | Popover::CommandMenu { active_id, .. } => active_id.clone(),
            Popover::Closed => None,
        };
        if active_id.is_none() {
            return unchanged(state, true);
        }
        return Transition {
            state: state.clone(),
            commands: vec![Command::SuggestionSelect {
                id: active_id.expect("checked above"),
            }],
            handled: true,
        };
    }
    let direction: i64 = if key == "ArrowDown" || (ctrl && key == "n") {
        1
    } else if key == "ArrowUp" || (ctrl && key == "p") {
        -1
    } else {
        0
    };
    if direction == 0 || ids.is_empty() {
        return unchanged(state, false);
    }
    let current_active = match &state.popover {
        Popover::Context { active_id, .. }
        | Popover::CommandInline { active_id, .. }
        | Popover::CommandMenu { active_id, .. } => active_id.clone(),
        Popover::Closed => None,
    };
    let current = current_active
        .as_ref()
        .and_then(|id| ids.iter().position(|i| i == id));
    let len = ids.len() as i64;
    let index = match current {
        None => {
            if direction == 1 {
                0
            } else {
                (len - 1) as usize
            }
        }
        Some(c) => ((c as i64 + direction + len) % len) as usize,
    };
    changed(
        &mut clone_with(state, |s| {
            set_popover_active(&mut s.popover, Some(ids[index].clone()))
        }),
        vec![],
        true,
    )
}

fn prompt_text(persisted: &PersistedState) -> String {
    persisted
        .prompt
        .iter()
        .map(|part| match part {
            PromptPart::Text { content } => content.as_str(),
            _ => "",
        })
        .collect()
}

fn populated(persisted: &PersistedState) -> bool {
    !prompt_text(persisted).trim().is_empty()
        || !persisted.context.items.is_empty()
        || persisted
            .prompt
            .iter()
            .any(|p| matches!(p, PromptPart::File { .. } | PromptPart::Image { .. }))
}

fn replace_trigger(value: &str, trigger: char, replacement: &str) -> String {
    let index = if trigger == '/' {
        value.find(trigger)
    } else {
        value.rfind(trigger)
    };
    match index {
        None => replacement.to_string(),
        Some(i) => format!("{}{}", &value[..i], replacement),
    }
}

fn changed(state: &mut InteractionState, commands: Vec<Command>, handled: bool) -> Transition {
    Transition {
        state: std::mem::replace(state, create_interaction_state()),
        commands,
        handled,
    }
}

fn unchanged(state: &InteractionState, handled: bool) -> Transition {
    Transition {
        state: state.clone(),
        commands: vec![],
        handled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_matches_reference() {
        let s = create_interaction_state();
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.popover, Popover::Closed);
        assert_eq!(s.drag, Drag::Idle);
        assert_eq!(s.focus, Focus::External);
        assert_eq!(s.history_index, -1);
    }

    #[test]
    fn exclamation_in_normal_mode_switches_to_shell() {
        let s = create_interaction_state();
        let persisted = PersistedState::default();
        let t = transition(
            &s,
            &Event::InputChanged {
                value: "!".into(),
                persist: None,
            },
            &persisted,
        );
        assert_eq!(t.state.mode, Mode::Shell);
        assert_eq!(t.state.popover, Popover::Closed);
        assert_eq!(
            t.commands,
            vec![Command::DraftSetText {
                value: String::new()
            }]
        );
    }

    #[test]
    fn at_trigger_opens_context_popover() {
        let s = create_interaction_state();
        let persisted = PersistedState::default();
        let t = transition(
            &s,
            &Event::InputChanged {
                value: "fix @par".into(),
                persist: None,
            },
            &persisted,
        );
        assert_eq!(
            t.state.popover,
            Popover::Context {
                query: "par".into(),
                active_id: None
            }
        );
    }

    #[test]
    fn slash_opens_command_inline() {
        let s = create_interaction_state();
        let persisted = PersistedState::default();
        let t = transition(
            &s,
            &Event::InputChanged {
                value: "/fix".into(),
                persist: None,
            },
            &persisted,
        );
        assert_eq!(
            t.state.popover,
            Popover::CommandInline {
                query: "fix".into(),
                active_id: None
            }
        );
    }

    #[test]
    fn escape_in_shell_returns_to_normal() {
        let mut s = create_interaction_state();
        s.mode = Mode::Shell;
        let t = transition(
            &s,
            &Event::KeyDown {
                key: "Escape".into(),
                ctrl: false,
                composing: false,
                ids: vec![],
                empty: None,
            },
            &PersistedState::default(),
        );
        assert_eq!(t.state.mode, Mode::Normal);
        assert!(t.handled);
    }

    #[test]
    fn arrow_down_wraps_around_ids() {
        let mut s = create_interaction_state();
        s.popover = Popover::CommandMenu {
            query: String::new(),
            active_id: Some("c".into()),
        };
        let t = transition(
            &s,
            &Event::KeyDown {
                key: "ArrowDown".into(),
                ctrl: false,
                composing: false,
                ids: vec!["a".into(), "b".into(), "c".into()],
                empty: None,
            },
            &PersistedState::default(),
        );
        assert_eq!(
            t.state.popover,
            Popover::CommandMenu {
                query: String::new(),
                active_id: Some("a".into())
            }
        );
        assert!(t.handled);
    }
}
