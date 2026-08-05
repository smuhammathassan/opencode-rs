//! Command registry.
//!
//! From reference/packages/core/src/command.ts.
//! Mirrors `CommandV2.Service`, backed by [`crate::state::State`].

use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::agent::ModelRef;
use crate::state::State;

/// `Command.Info` — `{ name, template, description?, agent?, model?,
/// subtask? }`.
/// From reference/packages/schema/src/command.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask: Option<bool>,
}

/// Command registry data (draft methods are inherent to this type).
#[derive(Debug, Clone, Default)]
pub struct CommandData {
    pub commands: IndexMap<String, CommandInfo>,
}

impl CommandData {
    pub fn list(&self) -> Vec<CommandInfo> {
        self.commands.values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<CommandInfo> {
        self.commands.get(name).cloned()
    }

    pub fn update(&mut self, name: &str, f: impl FnOnce(&mut CommandInfo)) {
        let entry = self
            .commands
            .entry(name.to_string())
            .or_insert_with(|| CommandInfo {
                name: name.to_string(),
                template: String::new(),
                description: None,
                agent: None,
                model: None,
                subtask: None,
            });
        f(entry);
        entry.name = name.to_string();
    }

    pub fn remove(&mut self, name: &str) {
        self.commands.shift_remove(name);
    }
}

/// The command service (`@opencode/v2/Command`).
#[derive(Clone)]
pub struct CommandService {
    state: Arc<State<CommandData>>,
}

impl CommandService {
    pub fn new() -> Self {
        CommandService {
            state: Arc::new(State::create(CommandData::default(), None)),
        }
    }

    pub fn state(&self) -> &Arc<State<CommandData>> {
        &self.state
    }

    pub async fn get(&self, name: &str) -> Option<CommandInfo> {
        self.state.get().commands.get(name).cloned()
    }

    pub async fn list(&self) -> Vec<CommandInfo> {
        self.state.get().commands.values().cloned().collect()
    }
}

impl Default for CommandService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_json_shape() {
        let info = CommandInfo {
            name: "test".to_string(),
            template: "run {file}".to_string(),
            description: Some("a command".to_string()),
            agent: None,
            model: None,
            subtask: None,
        };
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({
                "name": "test",
                "template": "run {file}",
                "description": "a command"
            })
        );
    }
}
