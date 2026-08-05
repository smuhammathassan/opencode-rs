//! Policy evaluation service.
//! From reference/packages/core/src/policy.ts

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::util::wildcard::wildcard_match;

/// `Policy.Effect` — "allow" | "deny".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

impl Effect {
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Allow => "allow",
            Effect::Deny => "deny",
        }
    }
}

/// `Policy.Info` — `{ action, effect, resource }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyInfo {
    pub action: String,
    pub effect: String,
    pub resource: String,
}

/// The policy service (`@opencode/v2/Policy`).
#[derive(Clone, Default)]
pub struct PolicyService {
    statements: Arc<Mutex<Vec<PolicyInfo>>>,
}

impl PolicyService {
    pub fn new() -> Self {
        PolicyService::default()
    }

    /// `Policy.load(statements)`.
    pub async fn load(&self, statements: Vec<PolicyInfo>) {
        *self.statements.lock().unwrap() = statements;
    }

    pub fn has_statements(&self) -> bool {
        !self.statements.lock().unwrap().is_empty()
    }

    /// `Policy.evaluate(action, resource, fallback)`.
    pub async fn evaluate(&self, action: &str, resource: &str, fallback: Effect) -> Effect {
        self.statements
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|statement| {
                wildcard_match(action, &statement.action)
                    && wildcard_match(resource, &statement.resource)
            })
            .map(|statement| {
                if statement.effect == "deny" {
                    Effect::Deny
                } else {
                    Effect::Allow
                }
            })
            .unwrap_or(fallback)
    }
}
