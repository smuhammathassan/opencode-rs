//! Prompt history (JSONL persistence).
//! From reference/packages/tui/src/prompt/history.tsx

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const MAX_HISTORY_ENTRIES: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PromptInfo {
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default)]
    pub parts: Vec<serde_json::Value>,
}

impl PromptInfo {
    pub fn new(input: impl Into<String>) -> Self {
        PromptInfo {
            input: input.into(),
            mode: None,
            parts: Vec::new(),
        }
    }
    pub fn effective_mode(&self) -> &str {
        self.mode.as_deref().unwrap_or("normal")
    }
}

/// Parse the JSONL history file, dropping corrupt lines.
/// From reference/packages/tui/src/prompt/history.tsx (`parsePromptHistory`)
pub fn parse_prompt_history(text: &str) -> Vec<PromptInfo> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<PromptInfo>(line).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(MAX_HISTORY_ENTRIES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// From reference/packages/tui/src/prompt/history.tsx (`isDuplicateEntry`)
pub fn is_duplicate_entry(previous: Option<&PromptInfo>, next: &PromptInfo) -> bool {
    match previous {
        Some(prev) => serde_json::to_string(prev).ok() == serde_json::to_string(next).ok(),
        None => false,
    }
}

/// In-memory history store with `move` semantics matching the reference.
#[derive(Debug, Clone, Default)]
pub struct PromptHistory {
    entries: Vec<PromptInfo>,
    /// Signed position; 0 = newest edge (the live draft), -1 = newest entry.
    index: i32,
}

impl PromptHistory {
    pub fn new(entries: Vec<PromptInfo>) -> Self {
        PromptHistory { entries, index: 0 }
    }

    pub fn entries(&self) -> &[PromptInfo] {
        &self.entries
    }

    /// Move through history relative to the current draft.
    /// `input` is the current plain text; when it differs from the current
    /// position the movement is ignored (mirrors `history.move`).
    /// From reference/packages/tui/src/prompt/history.tsx (`move`)
    pub fn move_dir(&mut self, direction: i32, input: &str) -> Option<PromptInfo> {
        if self.entries.is_empty() {
            return None;
        }
        // `index` is signed: 0 = newest edge (the live draft), -1 = newest
        // entry, -k = k-th newest entry.
        let current = if self.index == 0 {
            self.entries.first()
        } else {
            let pos = (self.entries.len() as i64 + self.index as i64) as usize;
            self.entries.get(pos)
        };
        if let Some(current) = current {
            if current.input != input && !input.is_empty() {
                return None;
            }
        }
        let next = self.index + direction;
        if next.unsigned_abs() as usize > self.entries.len() {
            return None;
        }
        if next > 0 {
            return None;
        }
        self.index = next;
        if self.index == 0 {
            return Some(PromptInfo::default());
        }
        self.entries
            .get((self.entries.len() as i64 + self.index as i64) as usize)
            .cloned()
    }

    pub fn move_previous(&mut self, input: &str) -> Option<PromptInfo> {
        self.move_dir(-1, input)
    }

    pub fn move_next(&mut self, input: &str) -> Option<PromptInfo> {
        self.move_dir(1, input)
    }

    /// Append an entry, trimming to the max and resetting the cursor.
    /// From reference/packages/tui/src/prompt/history.tsx (`append`)
    pub fn append(&mut self, item: PromptInfo) {
        if is_duplicate_entry(self.entries.last(), &item) {
            self.index = 0;
            return;
        }
        self.entries.push(item);
        if self.entries.len() > MAX_HISTORY_ENTRIES {
            let keep = self.entries.len() - MAX_HISTORY_ENTRIES;
            self.entries.drain(..keep);
        }
        self.index = 0;
    }

    /// Persist to a JSONL file.
    pub fn write(&self, path: &PathBuf) -> std::io::Result<()> {
        let mut text = String::new();
        for entry in &self.entries {
            if let Ok(json) = serde_json::to_string(entry) {
                text.push_str(&json);
                text.push('\n');
            }
        }
        std::fs::create_dir_all(path.parent().unwrap_or(PathBuf::from(".").as_path()))?;
        std::fs::write(path, text)
    }

    pub fn append_to_disk(&self, item: &PromptInfo, path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        if let Ok(json) = serde_json::to_string(item) {
            writeln!(file, "{json}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(input: &str) -> PromptInfo {
        PromptInfo::new(input)
    }

    #[test]
    fn parse_history_skips_corrupt_lines() {
        let text = "{\"input\":\"hello\"}\nnot json\n{\"input\":\"world\",\"mode\":\"shell\"}\n";
        let parsed = parse_prompt_history(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].input, "hello");
        assert_eq!(parsed[1].effective_mode(), "shell");
    }

    #[test]
    fn move_previous_walks_history() {
        let mut h = PromptHistory::new(vec![entry("one"), entry("two")]);
        assert_eq!(h.move_previous("").unwrap().input, "two");
        assert_eq!(h.move_previous("two").unwrap().input, "one");
        assert_eq!(h.move_previous("one"), None);
    }

    #[test]
    fn move_next_returns_to_draft() {
        let mut h = PromptHistory::new(vec![entry("one"), entry("two")]);
        h.move_previous("");
        h.move_previous("two");
        assert_eq!(h.move_next("one").unwrap().input, "two");
        assert_eq!(h.move_next("two").unwrap().input, "");
    }

    #[test]
    fn move_ignored_when_input_differs() {
        let mut h = PromptHistory::new(vec![entry("one"), entry("two")]);
        assert_eq!(h.move_previous("not-in-history"), None);
    }

    #[test]
    fn append_dedupes_consecutive() {
        let mut h = PromptHistory::new(Vec::new());
        h.append(entry("same"));
        h.append(entry("same"));
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn append_trims_to_limit() {
        let mut h = PromptHistory::new(Vec::new());
        for i in 0..(MAX_HISTORY_ENTRIES + 5) {
            h.append(entry(&format!("prompt-{i}")));
        }
        assert_eq!(h.entries().len(), MAX_HISTORY_ENTRIES);
        assert_eq!(h.entries().first().unwrap().input, "prompt-5");
    }

    #[test]
    fn duplicate_check() {
        assert!(is_duplicate_entry(Some(&entry("a")), &entry("a")));
        assert!(!is_duplicate_entry(Some(&entry("a")), &entry("b")));
        assert!(!is_duplicate_entry(None, &entry("a")));
    }
}
