//! Agent registry.
//!
//! From reference/packages/core/src/agent.ts.
//! Mirrors `AgentV2.Service`, backed by [`crate::state::State`].

use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::ids::AgentId;
use crate::state::State;

/// `Agent.Color` — a hex color or a named keyword.
/// From reference/packages/schema/src/agent.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Color {
    Hex(String),
    Keyword(String),
}

/// `Provider.Request` — `{ headers, body }`.
/// From reference/packages/schema/src/provider.ts
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub headers: serde_json::Map<String, serde_json::Value>,
    pub body: serde_json::Map<String, serde_json::Value>,
}

/// `Model.Ref` — `{ id, providerID, variant? }`.
/// From reference/packages/schema/src/model.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub id: String,
    pub providerID: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// `Permission.Rule` — `{ action, resource, effect }`.
/// From reference/packages/schema/src/permission.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRule {
    pub action: String,
    pub resource: String,
    pub effect: String,
}

/// `Agent.Info`.
/// From reference/packages/schema/src/agent.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: AgentId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    pub request: ProviderRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub mode: String,
    pub hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u64>,
    pub permissions: Vec<PermissionRule>,
}

impl AgentInfo {
    /// Mirrors `Agent.Info.empty(id)`.
    pub fn empty(id: AgentId) -> Self {
        AgentInfo {
            id,
            model: None,
            request: ProviderRequest::default(),
            system: None,
            description: None,
            mode: "all".to_string(),
            hidden: false,
            color: None,
            steps: None,
            permissions: Vec::new(),
        }
    }
}

/// `Agent.Selection` — `{ id, info? }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub id: AgentId,
    pub info: Option<AgentInfo>,
}

/// The agent registry data (draft methods are inherent to this type).
#[derive(Debug, Clone, Default)]
pub struct AgentData {
    pub agents: IndexMap<AgentId, AgentInfo>,
    pub default: Option<AgentId>,
}

impl AgentData {
    pub fn list(&self) -> Vec<AgentInfo> {
        self.agents.values().cloned().collect()
    }

    pub fn get(&self, id: &AgentId) -> Option<AgentInfo> {
        self.agents.get(id).cloned()
    }

    pub fn set_default(&mut self, id: Option<AgentId>) {
        self.default = id;
    }

    pub fn update(&mut self, id: &AgentId, f: impl FnOnce(&mut AgentInfo)) {
        let entry = self
            .agents
            .entry(id.clone())
            .or_insert_with(|| AgentInfo::empty(id.clone()));
        f(entry);
        entry.id = id.clone();
    }

    pub fn remove(&mut self, id: &AgentId) {
        self.agents.shift_remove(id);
    }
}

fn selectable(agent: Option<&AgentInfo>) -> Option<AgentInfo> {
    agent
        .filter(|agent| agent.mode != "subagent" && !agent.hidden)
        .cloned()
}

fn selected_default(data: &AgentData) -> Option<AgentInfo> {
    if let Some(configured) = data.default.as_ref().and_then(|id| data.agents.get(id)) {
        if let Some(agent) = selectable(Some(configured)) {
            return Some(agent);
        }
    }
    if let Some(agent) = selectable(data.agents.get(&AgentId::make("build"))) {
        return Some(agent);
    }
    for agent in data.agents.values() {
        if let Some(agent) = selectable(Some(agent)) {
            return Some(agent);
        }
    }
    None
}

/// The default agent ID — `ID.make("build")`.
pub fn default_id() -> AgentId {
    AgentId::make("build")
}

/// The agent service (`@opencode/v2/Agent`).
#[derive(Clone)]
pub struct AgentService {
    state: Arc<State<AgentData>>,
}

impl AgentService {
    pub fn new() -> Self {
        AgentService {
            state: Arc::new(State::create(AgentData::default(), None)),
        }
    }

    pub fn state(&self) -> &Arc<State<AgentData>> {
        &self.state
    }

    pub async fn get(&self, id: &AgentId) -> Option<AgentInfo> {
        self.state.get().agents.get(id).cloned()
    }

    pub async fn default(&self) -> Option<AgentInfo> {
        selected_default(&self.state.get())
    }

    pub async fn resolve(&self, id: Option<&AgentId>) -> Option<AgentInfo> {
        match id {
            Some(id) => self
                .state
                .get()
                .agents
                .get(&AgentId::make(id.0.clone()))
                .cloned(),
            None => selected_default(&self.state.get()),
        }
    }

    pub async fn select(&self, id: Option<&AgentId>) -> Selection {
        match id {
            Some(id) => {
                let id = AgentId::make(id.0.clone());
                Selection {
                    info: self.state.get().agents.get(&id).cloned(),
                    id,
                }
            }
            None => {
                let info = selected_default(&self.state.get());
                Selection {
                    id: info
                        .as_ref()
                        .map(|info| info.id.clone())
                        .unwrap_or_else(default_id),
                    info,
                }
            }
        }
    }

    pub async fn all(&self) -> Vec<AgentInfo> {
        self.state.get().agents.values().cloned().collect()
    }
}

impl Default for AgentService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_agent_json() {
        let info = AgentInfo::empty(AgentId::make("build"));
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({
                "id": "build",
                "request": { "headers": {}, "body": {} },
                "mode": "all",
                "hidden": false,
                "permissions": []
            })
        );
    }

    #[tokio::test]
    async fn select_falls_back_to_build() {
        let service = AgentService::new();
        let selection = service.select(None).await;
        assert_eq!(selection.id, AgentId::make("build"));
        assert!(selection.info.is_none());
    }

    #[tokio::test]
    async fn default_picks_first_selectable() {
        let mut next = AgentData::default();
        next.update(&AgentId::make("first"), |agent| {
            agent.mode = "primary".to_string()
        });
        next.update(&AgentId::make("hidden"), |agent| {
            agent.mode = "primary".to_string();
            agent.hidden = true;
        });
        next.update(&AgentId::make("sub"), |agent| {
            agent.mode = "subagent".to_string()
        });
        next.update(&AgentId::make("second"), |agent| {
            agent.mode = "primary".to_string()
        });
        assert_eq!(selected_default(&next).unwrap().id, AgentId::make("first"));

        // A configured default beats iteration order when selectable.
        next.set_default(Some(AgentId::make("second")));
        assert_eq!(selected_default(&next).unwrap().id, AgentId::make("second"));

        // A hidden configured default falls through.
        next.set_default(Some(AgentId::make("hidden")));
        assert_eq!(selected_default(&next).unwrap().id, AgentId::make("first"));
    }
}
