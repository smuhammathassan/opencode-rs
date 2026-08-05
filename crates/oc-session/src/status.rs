/// From reference/packages/opencode/src/session/status.ts and
/// reference/packages/schema/src/session-status-event.ts
///
/// Per-session runtime status (idle / busy / retry) kept in instance state.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Info {
    Idle,
    Busy,
    Retry {
        attempt: u64,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<Action>,
        next: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub reason: String,
    pub provider: String,
    pub title: String,
    pub message: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// In-memory status registry keyed by session id.
#[derive(Debug, Default)]
pub struct StatusMap {
    inner: std::collections::HashMap<String, Info>,
}

impl StatusMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, session_id: &str) -> Info {
        self.inner.get(session_id).cloned().unwrap_or(Info::Idle)
    }

    pub fn list(&self) -> std::collections::HashMap<String, Info> {
        self.inner.clone()
    }

    /// From reference `status.ts:set` — idle removes the entry.
    pub fn set(&mut self, session_id: &str, status: Info) {
        if status == Info::Idle {
            self.inner.remove(session_id);
        } else {
            self.inner.insert(session_id.to_string(), status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_default() {
        let map = StatusMap::new();
        assert_eq!(map.get("ses1"), Info::Idle);
    }

    #[test]
    fn busy_then_idle_removes_entry() {
        let mut map = StatusMap::new();
        map.set("ses1", Info::Busy);
        assert_eq!(map.get("ses1"), Info::Busy);
        map.set("ses1", Info::Idle);
        assert_eq!(map.get("ses1"), Info::Idle);
        assert!(map.inner.is_empty());
    }

    #[test]
    fn retry_info_serializes() {
        let info = Info::Retry {
            attempt: 2,
            message: "overloaded".into(),
            action: None,
            next: 10_000,
        };
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({ "type": "retry", "attempt": 2, "message": "overloaded", "next": 10000 })
        );
    }
}
