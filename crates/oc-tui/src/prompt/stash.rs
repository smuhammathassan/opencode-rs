//! Prompt stash (JSONL persistence).
//! From reference/packages/tui/src/prompt/stash.tsx

use serde::{Deserialize, Serialize};

use crate::prompt::history::PromptInfo;

pub const MAX_STASH_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StashEntry {
    pub input: String,
    #[serde(default)]
    pub parts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default)]
    pub timestamp: i64,
}

impl StashEntry {
    pub fn to_prompt_info(&self) -> PromptInfo {
        PromptInfo {
            input: self.input.clone(),
            mode: self.mode.clone(),
            parts: self.parts.clone(),
        }
    }
}

/// From reference/packages/tui/src/prompt/stash.tsx (`parsePromptStash`)
pub fn parse_prompt_stash(text: &str) -> Vec<StashEntry> {
    let mut entries: Vec<StashEntry> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<StashEntry>(line).ok())
        .collect();
    if entries.len() > MAX_STASH_ENTRIES {
        entries.drain(..entries.len() - MAX_STASH_ENTRIES);
    }
    entries
}

#[derive(Debug, Clone, Default)]
pub struct PromptStash {
    entries: Vec<StashEntry>,
}

impl PromptStash {
    pub fn new(entries: Vec<StashEntry>) -> Self {
        PromptStash { entries }
    }

    pub fn list(&self) -> &[StashEntry] {
        &self.entries
    }

    pub fn push(
        &mut self,
        input: impl Into<String>,
        parts: Vec<serde_json::Value>,
        mode: Option<String>,
    ) {
        self.entries.push(StashEntry {
            input: input.into(),
            parts,
            mode,
            timestamp: now_ms(),
        });
        if self.entries.len() > MAX_STASH_ENTRIES {
            let keep = self.entries.len() - MAX_STASH_ENTRIES;
            self.entries.drain(..keep);
        }
    }

    pub fn pop(&mut self) -> Option<StashEntry> {
        self.entries.pop()
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
        }
    }

    pub fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut text = String::new();
        for entry in &self.entries {
            if let Ok(json) = serde_json::to_string(entry) {
                text.push_str(&json);
                text.push('\n');
            }
        }
        std::fs::write(path, text)
    }

    pub fn append_to_disk(
        &self,
        entry: &StashEntry,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        if let Ok(json) = serde_json::to_string(entry) {
            writeln!(file, "{json}")?;
        }
        Ok(())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stash_skips_corrupt() {
        let text = "{\"input\":\"a\",\"parts\":[],\"timestamp\":1}\nbad\n";
        let parsed = parse_prompt_stash(text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].input, "a");
    }

    #[test]
    fn push_pop_trim() {
        let mut stash = PromptStash::default();
        for i in 0..(MAX_STASH_ENTRIES + 3) {
            stash.push(format!("item-{i}"), vec![], None);
        }
        assert_eq!(stash.list().len(), MAX_STASH_ENTRIES);
        assert_eq!(stash.list().first().unwrap().input, "item-3");
        let popped = stash.pop().unwrap();
        assert_eq!(popped.input, format!("item-{}", MAX_STASH_ENTRIES + 2));
    }

    #[test]
    fn remove_index() {
        let mut stash = PromptStash::new(parse_prompt_stash(
            "{\"input\":\"a\",\"parts\":[],\"timestamp\":1}\n{\"input\":\"b\",\"parts\":[],\"timestamp\":2}\n",
        ));
        stash.remove(0);
        assert_eq!(stash.list().len(), 1);
        assert_eq!(stash.list()[0].input, "b");
    }
}
