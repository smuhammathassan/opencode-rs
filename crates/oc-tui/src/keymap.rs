//! Keybinding parsing and matching.
//!
//! Port of the OpenTUI keymap layer configured by `reference/packages/tui/src/
//! keymap.tsx` + `config/keybind.ts`. Bindings are parsed from strings like
//! `ctrl+x`, `<leader>q`, `shift+return` into stroke sequences, then matched
//! against terminal key events. The `<leader>` key opens a timed chord so that
//! `ctrl+x` followed by `q` within the timeout dispatches the leader command.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode as CKeyCode, KeyEvent, KeyModifiers};

use crate::keybind::{LEADER_DEFAULT, LEADER_TIMEOUT_DEFAULT};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    F(u8),
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    Esc,
    Null,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub super_: bool,
    pub hyper: bool,
    pub code: KeyCode,
}

impl KeyStroke {
    pub fn plain(code: KeyCode) -> Self {
        KeyStroke {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            super_: false,
            hyper: false,
            code,
        }
    }

    pub fn display(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("ctrl".to_string());
        }
        if self.alt || self.meta {
            parts.push("alt".to_string());
        }
        if self.shift {
            parts.push("shift".to_string());
        }
        if self.super_ {
            parts.push("super".to_string());
        }
        if self.hyper {
            parts.push("hyper".to_string());
        }
        let name = match &self.code {
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Backspace => "backspace".to_string(),
            KeyCode::Enter => "return".to_string(),
            KeyCode::Left => "left".to_string(),
            KeyCode::Right => "right".to_string(),
            KeyCode::Up => "up".to_string(),
            KeyCode::Down => "down".to_string(),
            KeyCode::Home => "home".to_string(),
            KeyCode::End => "end".to_string(),
            KeyCode::PageUp => "pgup".to_string(),
            KeyCode::PageDown => "pgdn".to_string(),
            KeyCode::Tab => "tab".to_string(),
            KeyCode::BackTab => "shift+tab".to_string(),
            KeyCode::Delete => "del".to_string(),
            KeyCode::Insert => "insert".to_string(),
            KeyCode::Esc => "esc".to_string(),
            KeyCode::Null => "null".to_string(),
            KeyCode::Other(s) => s.clone(),
        };
        parts.push(name);
        parts.join("+")
    }
}

/// A single position in a key sequence: either the leader token or a stroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stroke {
    Leader,
    Key(KeyStroke),
}

/// One alternative binding (sequence of strokes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub strokes: Vec<Stroke>,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub command: String,
    pub desc: String,
    pub sequences: Vec<Sequence>,
    /// When true, the matched event is consumed even if another handler would
    /// have handled it (mirrors `preventDefault` in keybind objects).
    pub prevent_default: bool,
}

impl Binding {
    pub fn from_string(
        command: impl Into<String>,
        desc: impl Into<String>,
        raw: &str,
    ) -> Option<Binding> {
        let parsed = parse_binding(raw)?;
        Some(Binding {
            command: command.into(),
            desc: desc.into(),
            sequences: parsed.sequences,
            prevent_default: parsed.prevent_default,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ParsedBinding {
    pub sequences: Vec<Sequence>,
    pub prevent_default: bool,
}

/// Parse a keybind string into its alternative sequences.
/// Grammar: comma-separated alternatives; each alternative is a sequence of
/// strokes separated by spaces; strokes may carry modifiers joined with `+`.
/// `none` / `false` produce an empty (disabled) binding.
/// From reference/packages/tui/src/config/keybind.ts (`BindingValueSchema`)
pub fn parse_binding(input: &str) -> Option<ParsedBinding> {
    let input = input.trim();
    if input.is_empty() || input == "none" || input == "false" {
        return Some(ParsedBinding {
            sequences: Vec::new(),
            prevent_default: false,
        });
    }
    // Object form `{key:..., preventDefault:...}`.
    if input.starts_with('{') {
        let inner = &input[1..input.len().saturating_sub(1)];
        let mut key: Option<String> = None;
        let mut prevent_default = false;
        for part in inner.split(',') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix("key:") {
                key = Some(v.trim().to_string());
            } else if part.starts_with("preventDefault:") {
                let v = part.split(':').nth(1).unwrap_or("false").trim();
                prevent_default = v == "true";
            }
        }
        let key = key?;
        let mut parsed = parse_binding(&key)?;
        parsed.prevent_default = prevent_default;
        return Some(parsed);
    }

    let mut sequences = Vec::new();
    for alternative in input.split(',') {
        let alternative = alternative.trim();
        if alternative.is_empty() {
            continue;
        }
        let strokes = parse_sequence(alternative);
        if strokes.is_empty() {
            continue;
        }
        sequences.push(Sequence { strokes });
    }
    Some(ParsedBinding {
        sequences,
        prevent_default: false,
    })
}

fn parse_sequence(input: &str) -> Vec<Stroke> {
    let mut strokes = Vec::new();
    // Sequences are separated by spaces (except inside `<...>` tokens).
    let mut current = String::new();
    let mut angle_depth = 0usize;
    let flush = |current: &mut String, strokes: &mut Vec<Stroke>| {
        let token = current.trim();
        if token.is_empty() {
            return;
        }
        if let Some(stroke) = parse_stroke(token) {
            strokes.push(stroke);
        }
        current.clear();
    };
    for c in input.chars() {
        match c {
            '<' => {
                angle_depth += 1;
                current.push(c);
            }
            '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                current.push(c);
                if angle_depth == 0 {
                    flush(&mut current, &mut strokes);
                }
            }
            ' ' if angle_depth == 0 => flush(&mut current, &mut strokes),
            _ => current.push(c),
        }
    }
    if angle_depth > 0 {
        flush(&mut current, &mut strokes);
    }
    flush(&mut current, &mut strokes);
    strokes
}

fn parse_stroke(input: &str) -> Option<Stroke> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    // `<leader>` token.
    if input.starts_with('<') && input.ends_with('>') {
        let inner = input[1..input.len() - 1].to_ascii_lowercase();
        if inner == "leader" {
            return Some(Stroke::Leader);
        }
    }

    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut meta = false;
    let mut super_ = false;
    let mut hyper = false;
    let mut key = input;

    for part in input.split('+') {
        let part = part.trim().to_ascii_lowercase();
        match part.as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            "meta" => meta = true,
            "super" => super_ = true,
            "hyper" => hyper = true,
            _ => {}
        }
    }
    if ctrl || shift || alt || meta || super_ || hyper {
        // Last segment is the key.
        if let Some(idx) = input.rfind('+') {
            key = &input[idx + 1..];
        }
    }

    let code = parse_key_code(key)?;
    Some(Stroke::Key(KeyStroke {
        ctrl,
        shift,
        alt,
        meta,
        super_,
        hyper,
        code,
    }))
}

/// Key aliases: enter→return, esc→escape, pgdown→pagedown, pgup→pageup.
/// From reference/packages/tui/src/keymap.tsx (`KEY_ALIASES`)
fn parse_key_code(key: &str) -> Option<KeyCode> {
    let lower = key.to_ascii_lowercase();
    let code = match lower.as_str() {
        "return" | "enter" => KeyCode::Enter,
        "escape" | "esc" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "null" => KeyCode::Null,
        _ => {
            if let Some(num) = lower.strip_prefix('f').and_then(|s| s.parse::<u8>().ok()) {
                if (1..=24).contains(&num) {
                    KeyCode::F(num)
                } else {
                    return None;
                }
            } else if key.len() == 1 {
                KeyCode::Char(key.chars().next().unwrap())
            } else {
                return None;
            }
        }
    };
    Some(code)
}

impl KeyStroke {
    pub fn from_crossterm(event: &KeyEvent) -> KeyStroke {
        let m = event.modifiers;
        KeyStroke {
            ctrl: m.contains(KeyModifiers::CONTROL),
            shift: m.contains(KeyModifiers::SHIFT),
            alt: m.contains(KeyModifiers::ALT),
            meta: false,
            super_: m.contains(KeyModifiers::SUPER),
            hyper: false,
            code: match event.code {
                CKeyCode::Char(c) => KeyCode::Char(c),
                CKeyCode::F(n) => KeyCode::F(n),
                CKeyCode::Backspace => KeyCode::Backspace,
                CKeyCode::Enter => KeyCode::Enter,
                CKeyCode::Left => KeyCode::Left,
                CKeyCode::Right => KeyCode::Right,
                CKeyCode::Up => KeyCode::Up,
                CKeyCode::Down => KeyCode::Down,
                CKeyCode::Home => KeyCode::Home,
                CKeyCode::End => KeyCode::End,
                CKeyCode::PageUp => KeyCode::PageUp,
                CKeyCode::PageDown => KeyCode::PageDown,
                CKeyCode::Tab => KeyCode::Tab,
                CKeyCode::BackTab => KeyCode::BackTab,
                CKeyCode::Delete => KeyCode::Delete,
                CKeyCode::Insert => KeyCode::Insert,
                CKeyCode::Esc => KeyCode::Esc,
                CKeyCode::Null => KeyCode::Null,
                CKeyCode::CapsLock => KeyCode::Other("capslock".into()),
                CKeyCode::ScrollLock => KeyCode::Other("scrolllock".into()),
                CKeyCode::NumLock => KeyCode::Other("numlock".into()),
                CKeyCode::PrintScreen => KeyCode::Other("printscreen".into()),
                CKeyCode::Pause => KeyCode::Other("pause".into()),
                CKeyCode::Menu => KeyCode::Other("menu".into()),
                CKeyCode::KeypadBegin => KeyCode::Other("keypadbegin".into()),
                CKeyCode::Media(_) | CKeyCode::Modifier(_) => KeyCode::Other("modifier".into()),
            },
        }
    }

    /// Match this expected stroke against an actual keystroke. Char matching
    /// is case-sensitive except for ctrl-chords where terminals report the
    /// uppercase variant inconsistently.
    pub fn matches(&self, actual: &KeyStroke) -> bool {
        // Shift+Tab surfaces as BackTab with no SHIFT modifier.
        let actual_shift = actual.shift || actual.code == KeyCode::BackTab;
        let actual_code = if actual.code == KeyCode::BackTab {
            KeyCode::Tab
        } else {
            actual.code.clone()
        };
        let modifier_match = self.ctrl == actual.ctrl
            && self.alt == actual.alt
            && self.super_ == actual.super_
            && self.hyper == actual.hyper
            && self.meta == actual.meta
            && self.shift == actual_shift;
        if !modifier_match {
            return false;
        }
        match (&self.code, &actual_code) {
            (KeyCode::Char(expected), KeyCode::Char(actual_char)) => {
                if self.ctrl && self.shift {
                    expected.to_ascii_lowercase() == actual_char.to_ascii_lowercase()
                        || *expected == *actual_char
                } else if self.ctrl {
                    // Ctrl+letter may surface as control codes or as uppercase.
                    *expected == *actual_char
                        || expected.to_ascii_lowercase() == actual_char.to_ascii_lowercase()
                } else {
                    expected == actual_char
                }
            }
            (e, a) => e == a,
        }
    }
}

pub struct KeymapOptions {
    pub leader: String,
    pub leader_timeout: Duration,
}

impl Default for KeymapOptions {
    fn default() -> Self {
        KeymapOptions {
            leader: LEADER_DEFAULT.to_string(),
            leader_timeout: Duration::from_millis(LEADER_TIMEOUT_DEFAULT),
        }
    }
}

/// Ordered binding groups. Higher-priority groups are matched first.
#[derive(Debug, Clone)]
pub struct BindingGroup {
    pub priority: i32,
    pub enabled: bool,
    pub bindings: Vec<Binding>,
}

/// The keymap engine. Handles leader chords and dispatches commands.
pub struct Keymap {
    options: KeymapOptions,
    leader_stroke: Option<KeyStroke>,
    groups: Vec<BindingGroup>,
    /// Bindings whose sequence is in progress (leader chord).
    pending: Vec<PendingMatch>,
    pending_at: Instant,
    pub leader_active: bool,
}

struct PendingMatch {
    binding_index: usize,
    group_index: usize,
    stroke_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchResult {
    /// A command was dispatched.
    Command(String),
    /// The keystroke started/continued a pending leader sequence.
    Pending,
    /// No binding matched.
    None,
}

impl Keymap {
    pub fn new(options: KeymapOptions) -> Self {
        let leader_stroke = parse_binding(&options.leader)
            .and_then(|p| p.sequences.into_iter().next())
            .and_then(|seq| seq.strokes.into_iter().next())
            .and_then(|s| match s {
                Stroke::Key(k) => Some(k),
                Stroke::Leader => None,
            });
        Keymap {
            options,
            leader_stroke,
            groups: Vec::new(),
            pending: Vec::new(),
            pending_at: Instant::now(),
            leader_active: false,
        }
    }

    pub fn set_groups(&mut self, groups: Vec<BindingGroup>) {
        self.groups = groups;
    }

    pub fn clear_pending(&mut self) {
        self.pending.clear();
        self.leader_active = false;
    }

    /// Handle a keystroke. Returns the dispatched command, if any.
    pub fn handle(&mut self, event: &KeyEvent, now: Instant) -> MatchResult {
        let stroke = KeyStroke::from_crossterm(event);

        // Expire a stale leader chord.
        if self.leader_active && now.duration_since(self.pending_at) > self.options.leader_timeout {
            self.clear_pending();
        }

        // Advance pending sequences first.
        if self.leader_active {
            let mut completed: Option<(usize, usize)> = None;
            let mut advanced: Vec<PendingMatch> = Vec::new();
            for pending in self.pending.drain(..) {
                if let Some(group) = self.groups.get(pending.group_index) {
                    if let Some(binding) = group.bindings.get(pending.binding_index) {
                        if let Some(seq) = binding.sequences.first() {
                            if let Some(stroke_pos) = seq.strokes.get(pending.stroke_index) {
                                match stroke_pos {
                                    Stroke::Key(expected) if expected.matches(&stroke) => {
                                        if pending.stroke_index + 1 >= seq.strokes.len() {
                                            completed =
                                                Some((pending.group_index, pending.binding_index));
                                        } else {
                                            advanced.push(PendingMatch {
                                                stroke_index: pending.stroke_index + 1,
                                                ..pending
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            if let Some((group_index, binding_index)) = completed {
                self.clear_pending();
                if let Some(binding) = self
                    .groups
                    .get(group_index)
                    .and_then(|g| g.bindings.get(binding_index))
                {
                    return MatchResult::Command(binding.command.clone());
                }
                return MatchResult::None;
            }
            self.pending = advanced;
            if !self.pending.is_empty() {
                return MatchResult::Pending;
            }
            self.clear_pending();
            // Fall through to fresh matching for the same stroke.
        }

        // Fresh match across groups (highest priority first). A single-key
        // match wins immediately; a leader stroke opens a chord for every
        // leader-prefixed binding it can continue.
        let mut ordered: Vec<usize> = (0..self.groups.len()).collect();
        ordered.sort_by_key(|&i| std::cmp::Reverse(self.groups[i].priority));
        let mut pending: Vec<PendingMatch> = Vec::new();
        for group_index in ordered {
            let group = &self.groups[group_index];
            if !group.enabled {
                continue;
            }
            for (binding_index, binding) in group.bindings.iter().enumerate() {
                for seq in &binding.sequences {
                    if seq.strokes.is_empty() {
                        continue;
                    }
                    match &seq.strokes[0] {
                        Stroke::Key(expected) if expected.matches(&stroke) => {
                            if seq.strokes.len() == 1 {
                                return MatchResult::Command(binding.command.clone());
                            }
                        }
                        Stroke::Leader if seq.strokes.len() > 1 => {
                            if let Some(leader) = &self.leader_stroke {
                                if leader.matches(&stroke) {
                                    pending.push(PendingMatch {
                                        binding_index,
                                        group_index,
                                        stroke_index: 1,
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if !pending.is_empty() {
            self.pending = pending;
            self.pending_at = now;
            self.leader_active = true;
            return MatchResult::Pending;
        }
        MatchResult::None
    }
}

/// The leader key name for display.
pub fn leader_key_name() -> String {
    match parse_binding(LEADER_DEFAULT).and_then(|p| p.sequences.into_iter().next()) {
        Some(seq) => match seq.strokes.first() {
            Some(Stroke::Key(k)) => k.display(),
            Some(Stroke::Leader) => "leader".to_string(),
            None => LEADER_DEFAULT.to_string(),
        },
        None => LEADER_DEFAULT.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(to_crossterm(code), KeyModifiers::NONE)
    }
    fn mod_key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(to_crossterm(code), m)
    }
    fn to_crossterm(code: KeyCode) -> CKeyCode {
        match code {
            KeyCode::Char(c) => CKeyCode::Char(c),
            KeyCode::F(n) => CKeyCode::F(n),
            KeyCode::Backspace => CKeyCode::Backspace,
            KeyCode::Enter => CKeyCode::Enter,
            KeyCode::Left => CKeyCode::Left,
            KeyCode::Right => CKeyCode::Right,
            KeyCode::Up => CKeyCode::Up,
            KeyCode::Down => CKeyCode::Down,
            KeyCode::Home => CKeyCode::Home,
            KeyCode::End => CKeyCode::End,
            KeyCode::PageUp => CKeyCode::PageUp,
            KeyCode::PageDown => CKeyCode::PageDown,
            KeyCode::Tab => CKeyCode::Tab,
            KeyCode::BackTab => CKeyCode::BackTab,
            KeyCode::Delete => CKeyCode::Delete,
            KeyCode::Insert => CKeyCode::Insert,
            KeyCode::Esc => CKeyCode::Esc,
            KeyCode::Null => CKeyCode::Null,
            KeyCode::Other(_) => CKeyCode::Null,
        }
    }

    fn simple_binding(command: &str, raw: &str) -> Binding {
        Binding::from_string(command, "", raw).unwrap()
    }

    #[test]
    fn parse_single_keys() {
        let b = parse_binding("ctrl+x").unwrap();
        assert_eq!(b.sequences.len(), 1);
        assert_eq!(
            b.sequences[0].strokes,
            vec![Stroke::Key(KeyStroke {
                ctrl: true,
                shift: false,
                alt: false,
                meta: false,
                super_: false,
                hyper: false,
                code: KeyCode::Char('x'),
            })]
        );
    }

    #[test]
    fn parse_comma_alternatives() {
        let b = parse_binding("up,ctrl+p").unwrap();
        assert_eq!(b.sequences.len(), 2);
        assert_eq!(
            b.sequences[0].strokes,
            vec![Stroke::Key(KeyStroke::plain(KeyCode::Up))]
        );
        assert_eq!(b.sequences[1].strokes.len(), 1);
    }

    #[test]
    fn parse_leader_sequence() {
        let b = parse_binding("<leader>e").unwrap();
        assert_eq!(b.sequences.len(), 1);
        assert_eq!(
            b.sequences[0].strokes,
            vec![
                Stroke::Leader,
                Stroke::Key(KeyStroke::plain(KeyCode::Char('e')))
            ]
        );
    }

    #[test]
    fn parse_none_disables() {
        let b = parse_binding("none").unwrap();
        assert!(b.sequences.is_empty());
        let b = parse_binding("false").unwrap();
        assert!(b.sequences.is_empty());
    }

    #[test]
    fn parse_object_form() {
        let b = parse_binding("{key:ctrl+v,preventDefault:false}").unwrap();
        assert_eq!(b.sequences.len(), 1);
        assert!(!b.prevent_default);
    }

    #[test]
    fn parse_key_aliases() {
        assert_eq!(
            parse_binding("enter").unwrap().sequences[0].strokes[0],
            Stroke::Key(KeyStroke::plain(KeyCode::Enter))
        );
        assert_eq!(
            parse_binding("esc").unwrap().sequences[0].strokes[0],
            Stroke::Key(KeyStroke::plain(KeyCode::Esc))
        );
        assert_eq!(
            parse_binding("pgup").unwrap().sequences[0].strokes[0],
            Stroke::Key(KeyStroke::plain(KeyCode::PageUp))
        );
    }

    #[test]
    fn parse_shift_enter() {
        let b = parse_binding("shift+return").unwrap();
        assert_eq!(
            b.sequences[0].strokes[0],
            Stroke::Key(KeyStroke {
                ctrl: false,
                shift: true,
                alt: false,
                meta: false,
                super_: false,
                hyper: false,
                code: KeyCode::Enter,
            })
        );
    }

    #[test]
    fn parse_function_keys() {
        assert_eq!(
            parse_binding("f2").unwrap().sequences[0].strokes[0],
            Stroke::Key(KeyStroke::plain(KeyCode::F(2)))
        );
        assert_eq!(
            parse_binding("shift+f2").unwrap().sequences[0].strokes[0],
            Stroke::Key(KeyStroke {
                shift: true,
                ..KeyStroke::plain(KeyCode::F(2))
            })
        );
    }

    #[test]
    fn ctrl_char_matches_uppercase_surface() {
        let binding = KeyStroke {
            ctrl: true,
            code: KeyCode::Char('c'),
            ..KeyStroke::plain(KeyCode::Char('c'))
        };
        let actual = KeyStroke {
            ctrl: true,
            code: KeyCode::Char('C'),
            ..KeyStroke::plain(KeyCode::Char('C'))
        };
        assert!(binding.matches(&actual));
    }

    #[test]
    fn plain_char_case_sensitive() {
        let binding = KeyStroke::plain(KeyCode::Char('q'));
        assert!(binding.matches(&KeyStroke::plain(KeyCode::Char('q'))));
        assert!(!binding.matches(&KeyStroke::plain(KeyCode::Char('Q'))));
    }

    #[test]
    fn keymap_dispatches_single_binding() {
        let mut km = Keymap::new(KeymapOptions::default());
        km.set_groups(vec![BindingGroup {
            priority: 0,
            enabled: true,
            bindings: vec![simple_binding("command.palette.show", "ctrl+p")],
        }]);
        let now = Instant::now();
        assert_eq!(
            km.handle(&mod_key(KeyCode::Char('p'), KeyModifiers::CONTROL), now),
            MatchResult::Command("command.palette.show".to_string())
        );
        assert_eq!(km.handle(&key(KeyCode::Char('p')), now), MatchResult::None);
    }

    #[test]
    fn keymap_leader_chord() {
        let mut km = Keymap::new(KeymapOptions::default());
        km.set_groups(vec![BindingGroup {
            priority: 0,
            enabled: true,
            bindings: vec![
                simple_binding("app.exit", "<leader>q"),
                simple_binding("model.list", "<leader>m"),
            ],
        }]);
        let now = Instant::now();
        // ctrl+x opens the chord.
        assert_eq!(
            km.handle(&mod_key(KeyCode::Char('x'), KeyModifiers::CONTROL), now),
            MatchResult::Pending
        );
        assert!(km.leader_active);
        // q completes it.
        assert_eq!(
            km.handle(&key(KeyCode::Char('q')), now + Duration::from_millis(10)),
            MatchResult::Command("app.exit".to_string())
        );
        assert!(!km.leader_active);
    }

    #[test]
    fn keymap_leader_times_out() {
        let mut km = Keymap::new(KeymapOptions {
            leader_timeout: Duration::from_millis(100),
            ..KeymapOptions::default()
        });
        km.set_groups(vec![BindingGroup {
            priority: 0,
            enabled: true,
            bindings: vec![simple_binding("app.exit", "<leader>q")],
        }]);
        let now = Instant::now();
        assert_eq!(
            km.handle(&mod_key(KeyCode::Char('x'), KeyModifiers::CONTROL), now),
            MatchResult::Pending
        );
        // After the timeout the same key falls through and matches nothing.
        assert_eq!(
            km.handle(&key(KeyCode::Char('q')), now + Duration::from_millis(200)),
            MatchResult::None
        );
    }

    #[test]
    fn priority_wins() {
        let mut km = Keymap::new(KeymapOptions::default());
        km.set_groups(vec![
            BindingGroup {
                priority: 0,
                enabled: true,
                bindings: vec![simple_binding("low", "ctrl+c")],
            },
            BindingGroup {
                priority: 10,
                enabled: true,
                bindings: vec![simple_binding("high", "ctrl+c")],
            },
        ]);
        assert_eq!(
            km.handle(
                &mod_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                Instant::now()
            ),
            MatchResult::Command("high".to_string())
        );
    }

    #[test]
    fn disabled_group_skipped() {
        let mut km = Keymap::new(KeymapOptions::default());
        km.set_groups(vec![
            BindingGroup {
                priority: 10,
                enabled: false,
                bindings: vec![simple_binding("disabled", "ctrl+c")],
            },
            BindingGroup {
                priority: 0,
                enabled: true,
                bindings: vec![simple_binding("enabled", "ctrl+c")],
            },
        ]);
        assert_eq!(
            km.handle(
                &mod_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                Instant::now()
            ),
            MatchResult::Command("enabled".to_string())
        );
    }

    #[test]
    fn multiple_leader_bindings_share_chord() {
        let mut km = Keymap::new(KeymapOptions::default());
        km.set_groups(vec![BindingGroup {
            priority: 0,
            enabled: true,
            bindings: vec![
                simple_binding("cmd1", "<leader>a"),
                simple_binding("cmd2", "<leader>b"),
            ],
        }]);
        let now = Instant::now();
        assert_eq!(
            km.handle(&mod_key(KeyCode::Char('x'), KeyModifiers::CONTROL), now),
            MatchResult::Pending
        );
        assert_eq!(
            km.handle(&key(KeyCode::Char('b')), now + Duration::from_millis(5)),
            MatchResult::Command("cmd2".to_string())
        );
    }

    #[test]
    fn backtab_matches_shift_tab() {
        let binding = KeyStroke {
            shift: true,
            ..KeyStroke::plain(KeyCode::Tab)
        };
        assert!(binding.matches(&KeyStroke::plain(KeyCode::BackTab)));
    }
}
