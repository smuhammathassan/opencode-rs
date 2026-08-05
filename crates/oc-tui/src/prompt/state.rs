//! Prompt runtime state: buffer + parts + mode + autocomplete.
//!
//! Backs `reference/packages/tui/src/component/prompt/index.tsx` and
//! `component/prompt/autocomplete.tsx`.

use serde_json::Value;

use crate::prompt::autocomplete::{mention_query, Trigger};
use crate::prompt::input::TextBuffer;
use crate::prompt::parts::sync_part_ranges;
use crate::types::Agent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptMode {
    #[default]
    Normal,
    Shell,
}

impl PromptMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromptMode::Normal => "normal",
            PromptMode::Shell => "shell",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insert {
    /// Replace from offset 0 with this text (slash commands).
    Slash(String),
    /// Insert text at the trigger offset.
    Mention(String),
    /// Attach a file/agent part and insert its marker.
    Part(Value),
}

#[derive(Debug, Clone)]
pub struct AutocompleteOption {
    pub display: String,
    pub description: Option<String>,
    pub insert: Insert,
    pub is_directory: bool,
}

impl AutocompleteOption {
    pub fn slash(command: &str, description: Option<String>) -> Self {
        AutocompleteOption {
            display: format!("/{command}"),
            description,
            insert: Insert::Slash(format!("/{command} ")),
            is_directory: false,
        }
    }

    pub fn agent(agent: &Agent) -> Self {
        AutocompleteOption {
            display: format!("@{}", agent.name),
            description: agent.description.clone(),
            insert: Insert::Mention(format!("@{} ", agent.name)),
            is_directory: false,
        }
    }

    pub fn file(display: String, marker: String, part: Value, is_directory: bool) -> Self {
        AutocompleteOption {
            display,
            description: None,
            insert: Insert::Part(part),
            is_directory,
        }
        .with_marker_memo(marker)
    }

    fn with_marker_memo(self, _marker: String) -> Self {
        self
    }
}

/// Autocomplete popup state.
#[derive(Debug, Clone, Default)]
pub struct AutocompleteState {
    pub visible: bool,
    pub trigger: Option<Trigger>,
    pub index: usize,
    pub selected: usize,
    pub options: Vec<AutocompleteOption>,
    pub file_query: String,
}

impl AutocompleteState {
    pub fn hidden() -> Self {
        AutocompleteState::default()
    }

    pub fn query<'a>(&self, buffer: &'a str) -> &'a str {
        match self.trigger {
            Some(Trigger::Mention) => {
                let offset = buffer.chars().count();
                let trigger = self.index;
                mention_query(buffer, offset, trigger)
            }
            Some(Trigger::Slash) => &buffer[1..buffer.len()],
            None => "",
        }
    }

    pub fn move_selection(&mut self, direction: i32) {
        if self.options.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected =
            (self.selected as i32 + direction).rem_euclid(self.options.len() as i32) as usize;
    }

    /// Insert the selected option into the buffer.
    pub fn selected_insert(&self) -> Option<Insert> {
        self.options.get(self.selected).map(|o| o.insert.clone())
    }
}

/// The prompt's runtime state.
#[derive(Debug, Clone)]
pub struct PromptState {
    pub buffer: TextBuffer,
    pub parts: Vec<Value>,
    pub mode: PromptMode,
    pub placeholder: usize,
    pub autocomplete: AutocompleteState,
    pub interrupt: u32,
}

impl Default for PromptState {
    fn default() -> Self {
        PromptState {
            buffer: TextBuffer::new(),
            parts: Vec::new(),
            mode: PromptMode::Normal,
            placeholder: 0,
            autocomplete: AutocompleteState::default(),
            interrupt: 0,
        }
    }
}

impl PromptState {
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Plain text with markers stripped.
    pub fn plain_text(&self) -> String {
        crate::prompt::parts::plain_text(&self.buffer.text(), &self.parts)
    }

    /// Insert a marker (e.g. pasted content) and register it as a part.
    pub fn insert_part(&mut self, marker: &str, part: Value) {
        let start = self.buffer.cursor();
        self.buffer.insert_str(&format!("{marker} "));
        let end = start + marker.chars().count();
        let mut part = part;
        if let Some(source) = part.get_mut("source").and_then(Value::as_object_mut) {
            if let Some(text) = source.get_mut("text").and_then(Value::as_object_mut) {
                text.insert("start".into(), Value::from(start));
                text.insert("end".into(), Value::from(end));
                text.insert("value".into(), Value::from(marker.to_string()));
            } else {
                source.insert("start".into(), Value::from(start));
                source.insert("end".into(), Value::from(end));
                source.insert("value".into(), Value::from(marker.to_string()));
            }
        }
        self.parts.push(part);
    }

    /// Apply the selected autocomplete insertion.
    pub fn apply_autocomplete(&mut self) -> bool {
        let Some(insert) = self.autocomplete.selected_insert() else {
            return false;
        };
        let trigger_offset = self.autocomplete.index;
        match insert {
            Insert::Slash(text) => {
                let end = self.buffer.cursor();
                self.buffer.delete_range(0, end);
                self.buffer.set_cursor(0);
                self.buffer.insert_str(&text);
            }
            Insert::Mention(text) => {
                // Replace the typed query between trigger and cursor.
                let query_len = self.buffer.cursor().saturating_sub(trigger_offset);
                self.buffer
                    .delete_range(trigger_offset, self.buffer.cursor());
                self.buffer.set_cursor(trigger_offset);
                let _ = query_len;
                self.buffer.insert_str(&text);
            }
            Insert::Part(part) => {
                let marker = crate::prompt::parts::source_text(&part)
                    .map(|(_, _, value)| value)
                    .unwrap_or_default();
                let insert_text = if marker.is_empty() {
                    "@file".to_string()
                } else {
                    marker
                };
                let query_len = self.buffer.cursor().saturating_sub(trigger_offset);
                self.buffer
                    .delete_range(trigger_offset, self.buffer.cursor());
                self.buffer.set_cursor(trigger_offset);
                let _ = query_len;
                let start = trigger_offset;
                self.buffer.insert_str(&insert_text);
                let mut part = part;
                if let Some(source) = part.get_mut("source").and_then(Value::as_object_mut) {
                    if let Some(text) = source.get_mut("text").and_then(Value::as_object_mut) {
                        text.insert("start".into(), Value::from(start));
                        text.insert(
                            "end".into(),
                            Value::from(start + insert_text.chars().count()),
                        );
                        text.insert("value".into(), Value::from(insert_text.clone()));
                    }
                }
                self.parts.push(part);
            }
        }
        self.autocomplete = AutocompleteState::default();
        self.sync_parts();
        true
    }

    /// Reconcile part markers with the buffer after an edit.
    pub fn sync_parts(&mut self) {
        let text = self.buffer.text();
        sync_part_ranges(&text, &mut self.parts);
    }

    /// Recompute the visible autocomplete trigger based on input.
    pub fn update_autocomplete(&mut self) {
        let value = self.buffer.text();
        let cursor = self.buffer.cursor();
        let ac = &mut self.autocomplete;
        if ac.visible {
            let hide = match ac.trigger {
                Some(Trigger::Mention) => {
                    cursor <= ac.index
                        || value[ac.index.min(value.len())..cursor.min(value.len())]
                            .chars()
                            .any(char::is_whitespace)
                }
                Some(Trigger::Slash) => {
                    cursor <= ac.index
                        || value[ac.index.min(value.len())..cursor.min(value.len())]
                            .chars()
                            .any(char::is_whitespace)
                }
                None => true,
            };
            if hide {
                *ac = AutocompleteState::default();
                return;
            }
        }
        if let Some(trigger) = crate::prompt::autocomplete::trigger_for_input(&value, cursor) {
            ac.visible = true;
            ac.trigger = Some(trigger);
            match trigger {
                Trigger::Mention => {
                    ac.index = crate::prompt::autocomplete::mention_trigger_index(&value, cursor)
                        .unwrap_or(0);
                }
                Trigger::Slash => {
                    ac.index = 0;
                }
            }
            ac.selected = 0;
        }
    }

    pub fn hide_autocomplete(&mut self) {
        self.autocomplete = AutocompleteState::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn insert_part_creates_marker_and_source() {
        let mut prompt = PromptState::default();
        prompt.buffer.insert_str("hello");
        prompt.buffer.set_cursor(5);
        let part = json!({
            "type": "file", "mime": "image/png", "filename": "a.png", "url": "x",
            "source": { "type": "file", "path": "a.png", "text": { "value": "", "start": 0, "end": 0 } }
        });
        prompt.insert_part("[Image 1]", part);
        assert_eq!(prompt.text(), "hello[Image 1] ");
        let (start, end, value) = crate::prompt::parts::source_text(&prompt.parts[0]).unwrap();
        assert_eq!((start, end), (5, 14));
        assert_eq!(value, "[Image 1]");
    }

    #[test]
    fn plain_text_strips_marker() {
        let mut prompt = PromptState::default();
        prompt.buffer.insert_str("hello[Image 1] world");
        let part = json!({
            "type": "file", "mime": "image/png", "filename": "a.png", "url": "x",
            "source": { "type": "file", "path": "a.png", "text": { "value": "[Image 1]", "start": 5, "end": 14 } }
        });
        prompt.parts.push(part);
        assert_eq!(prompt.plain_text(), "hello world");
    }

    #[test]
    fn mention_autocomplete_updates() {
        let mut prompt = PromptState::default();
        prompt.buffer.insert_str("@bu");
        prompt.buffer.set_cursor(3);
        prompt.update_autocomplete();
        assert!(prompt.autocomplete.visible);
        assert_eq!(prompt.autocomplete.trigger, Some(Trigger::Mention));
        prompt.autocomplete.options = vec![AutocompleteOption::agent(
            &serde_json::from_value(json!({
                "name": "build", "mode": "primary", "permission": [], "options": {}
            }))
            .unwrap(),
        )];
        prompt.apply_autocomplete();
        assert_eq!(prompt.text(), "@build ");
        assert!(!prompt.autocomplete.visible);
    }

    #[test]
    fn slash_autocomplete_replaces_from_start() {
        let mut prompt = PromptState::default();
        prompt.buffer.insert_str("/mo");
        prompt.buffer.set_cursor(3);
        prompt.update_autocomplete();
        assert_eq!(prompt.autocomplete.trigger, Some(Trigger::Slash));
        prompt.autocomplete.options = vec![AutocompleteOption::slash("models", None)];
        prompt.apply_autocomplete();
        assert_eq!(prompt.text(), "/models ");
    }

    #[test]
    fn editing_drops_deleted_parts() {
        let mut prompt = PromptState::default();
        prompt.buffer.insert_str("[Image 1] hi");
        let part = json!({
            "type": "file", "mime": "image/png", "filename": "a.png", "url": "x",
            "source": { "type": "file", "path": "a.png", "text": { "value": "[Image 1]", "start": 0, "end": 9 } }
        });
        prompt.parts.push(part);
        // Delete the marker.
        prompt.buffer.set_cursor(0);
        for _ in 0..9 {
            prompt.buffer.delete();
        }
        prompt.sync_parts();
        assert!(prompt.parts.is_empty());
    }
}
