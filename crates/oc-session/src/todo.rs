/// From reference/packages/opencode/src/session/todo.ts and
/// reference/packages/schema/src/session-todo.ts
///
/// Session todo list — `Todo.Info` schema plus update/get against the store.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// Brief description of the task
    pub content: String,
    /// Current status of the task: pending, in_progress, completed, cancelled
    pub status: String,
    /// Priority level of the task: high, medium, low
    pub priority: String,
}

/// Event payload: `todo.updated`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedEvent {
    pub session_id: String,
    pub todos: Vec<Info>,
}

/// From reference `todo.ts:update` — replaces the full todo list, preserving
/// order via `position`.
pub fn replace(existing: &[Info], todos: &[Info]) -> Vec<Info> {
    let mut next = todos.to_vec();
    next.sort_by_key(|todo| {
        existing
            .iter()
            .position(|item| item == todo)
            .unwrap_or(usize::MAX)
    });
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo(content: &str, status: &str, priority: &str) -> Info {
        Info {
            content: content.to_string(),
            status: status.to_string(),
            priority: priority.to_string(),
        }
    }

    #[test]
    fn info_serializes_with_reference_fields() {
        let value = serde_json::to_value(todo("Fix build", "in_progress", "high")).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "content": "Fix build",
                "status": "in_progress",
                "priority": "high"
            })
        );
    }
}
