//! Local UI selection state (current agent/model/variant).
//! Mirrors `reference/packages/tui/src/context/local.tsx`.

use crate::sync::SyncState;
use crate::types::Agent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub provider_id: String,
    pub model_id: String,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Local {
    pub agent: Option<String>,
    pub model: Option<ModelSelection>,
    pub recent_models: Vec<ModelSelection>,
    pub permission_mode: String,
}

impl Local {
    /// Primary (non-subagent, non-hidden) agents.
    pub fn primary_agents<'a>(&self, sync: &'a SyncState) -> Vec<&'a Agent> {
        sync.agents
            .iter()
            .filter(|a| a.mode != "subagent" && !a.hidden.unwrap_or(false))
            .collect()
    }

    pub fn all_agents<'a>(&self, sync: &'a SyncState) -> Vec<&'a Agent> {
        sync.agents
            .iter()
            .filter(|a| !a.hidden.unwrap_or(false))
            .collect()
    }

    pub fn current_agent<'a>(&self, sync: &'a SyncState) -> Option<&'a Agent> {
        let agents = self.primary_agents(sync);
        if agents.is_empty() {
            return None;
        }
        match &self.agent {
            Some(name) => agents
                .iter()
                .find(|a| a.name == *name)
                .copied()
                .or_else(|| agents.first().copied()),
            None => agents.first().copied(),
        }
    }

    pub fn set_agent(&mut self, name: &str) {
        self.agent = Some(name.to_string());
    }

    pub fn cycle_agent(&mut self, sync: &SyncState, direction: i32) {
        let agents = self.primary_agents(sync);
        if agents.is_empty() {
            return;
        }
        let current_idx = self
            .agent
            .as_deref()
            .and_then(|name| agents.iter().position(|a| a.name == *name))
            .unwrap_or(0);
        let mut next = (current_idx as i32 + direction).rem_euclid(agents.len() as i32);
        if next < 0 {
            next += agents.len() as i32;
        }
        self.agent = Some(agents[next as usize].name.clone());
    }

    pub fn current_model(&self, sync: &SyncState) -> Option<ModelSelection> {
        if let Some(model) = &self.model {
            if is_model_valid(sync, model) {
                return Some(model.clone());
            }
        }
        // Fall back to the agent's configured model, then the first provider model.
        let agent = self.current_agent(sync)?;
        if let Some(agent_model) = &agent.model {
            let selection = ModelSelection {
                provider_id: agent_model.provider_id.clone(),
                model_id: agent_model.id.clone(),
                variant: agent.variant.clone(),
            };
            if is_model_valid(sync, &selection) {
                return Some(selection);
            }
        }
        let provider = sync.providers.iter().find(|p| !p.models.is_empty())?;
        let model_id = provider.models.keys().min()?.clone();
        Some(ModelSelection {
            provider_id: provider.id.clone(),
            model_id,
            variant: None,
        })
    }

    pub fn set_model(&mut self, selection: ModelSelection, recent: bool) {
        if recent {
            self.recent_models.retain(|m| m != &selection);
            self.recent_models.push(selection.clone());
            if self.recent_models.len() > 20 {
                self.recent_models.remove(0);
            }
        }
        self.model = Some(selection);
    }

    pub fn cycle_model(&mut self, sync: &SyncState, direction: i32) {
        let current = self.current_model(sync);
        if current.is_none() {
            return;
        }
        let mut flat: Vec<ModelSelection> = Vec::new();
        for provider in &sync.providers {
            let mut ids: Vec<&String> = provider.models.keys().collect();
            ids.sort();
            for id in ids {
                flat.push(ModelSelection {
                    provider_id: provider.id.clone(),
                    model_id: id.clone(),
                    variant: None,
                });
            }
        }
        if flat.is_empty() {
            return;
        }
        let idx = current
            .as_ref()
            .and_then(|c| flat.iter().position(|m| m == c))
            .unwrap_or(0);
        let next = (idx as i32 + direction).rem_euclid(flat.len() as i32) as usize;
        self.set_model(flat[next].clone(), true);
    }

    pub fn cycle_recent_model(&mut self, direction: i32) -> Option<ModelSelection> {
        if self.recent_models.is_empty() {
            return None;
        }
        let idx = self
            .model
            .as_ref()
            .and_then(|m| self.recent_models.iter().position(|r| r == m))
            .unwrap_or(0);
        let next = (idx as i32 + direction).rem_euclid(self.recent_models.len() as i32) as usize;
        let selection = self.recent_models[next].clone();
        self.set_model(selection.clone(), false);
        Some(selection)
    }
}

fn is_model_valid(sync: &SyncState, selection: &ModelSelection) -> bool {
    sync.providers
        .iter()
        .find(|p| p.id == selection.provider_id)
        .is_some_and(|p| p.models.contains_key(&selection.model_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sync_with_provider() -> SyncState {
        let mut sync = SyncState::default();
        sync.providers = serde_json::from_value(json!([
            { "id": "p1", "name": "P1", "source": "config", "env": [], "options": {},
              "models": {
                "m1": { "id": "m1", "providerID": "p1", "name": "M1",
                    "capabilities": {"input": {}, "output": {}}, "cost": {"input": 0, "output": 0, "cache": {"read": 0, "write": 0}},
                    "limit": {"context": 10, "output": 10}, "status": "active", "options": {}, "headers": {}, "release_date": "" },
                "m2": { "id": "m2", "providerID": "p1", "name": "M2",
                    "capabilities": {"input": {}, "output": {}}, "cost": {"input": 0, "output": 0, "cache": {"read": 0, "write": 0}},
                    "limit": {"context": 10, "output": 10}, "status": "active", "options": {}, "headers": {}, "release_date": "" }
              } }
        ]))
        .unwrap();
        sync.agents = serde_json::from_value(json!([
            { "name": "build", "mode": "primary", "permission": {}, "options": {} },
            { "name": "plan", "mode": "primary", "permission": {}, "options": {} },
            { "name": "coder", "mode": "subagent", "permission": {}, "options": {} }
        ]))
        .unwrap();
        sync
    }

    #[test]
    fn primary_agents_exclude_subagents() {
        let sync = sync_with_provider();
        let local = Local::default();
        assert_eq!(local.primary_agents(&sync).len(), 2);
        assert_eq!(local.current_agent(&sync).unwrap().name, "build");
    }

    #[test]
    fn cycle_agent_wraps() {
        let sync = sync_with_provider();
        let mut local = Local::default();
        local.cycle_agent(&sync, 1);
        assert_eq!(local.current_agent(&sync).unwrap().name, "plan");
        local.cycle_agent(&sync, 1);
        assert_eq!(local.current_agent(&sync).unwrap().name, "build");
        local.cycle_agent(&sync, -1);
        assert_eq!(local.current_agent(&sync).unwrap().name, "plan");
    }

    #[test]
    fn current_model_falls_back_to_first_provider_model() {
        let sync = sync_with_provider();
        let local = Local::default();
        let model = local.current_model(&sync).unwrap();
        assert_eq!(model.provider_id, "p1");
        assert_eq!(model.model_id, "m1");
    }

    #[test]
    fn cycle_model() {
        let sync = sync_with_provider();
        let mut local = Local::default();
        local.cycle_model(&sync, 1);
        assert_eq!(local.current_model(&sync).unwrap().model_id, "m2");
        local.cycle_model(&sync, -1);
        assert_eq!(local.current_model(&sync).unwrap().model_id, "m1");
    }

    #[test]
    fn recent_models_cycle() {
        let _sync = sync_with_provider();
        let mut local = Local::default();
        local.set_model(
            ModelSelection {
                provider_id: "p1".into(),
                model_id: "m1".into(),
                variant: None,
            },
            true,
        );
        local.set_model(
            ModelSelection {
                provider_id: "p1".into(),
                model_id: "m2".into(),
                variant: None,
            },
            true,
        );
        assert_eq!(local.cycle_recent_model(1).unwrap().model_id, "m1");
    }
}
