//! Directory snapshot for ACP sessions.
//!
//! From reference/packages/opencode/src/acp/directory.ts. A snapshot captures
//! the providers/models, modes, commands, and default model for a directory.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use crate::error::ACPError;
use crate::sdk::{CommandInfo, ProviderInfo};

/// A model option presented in config options.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelOption {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
}

/// A session mode option.
#[derive(Debug, Clone, PartialEq)]
pub struct ModeOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// Model variants keyed by variant name. `IndexMap` preserves the reference's
/// `Object.values` iteration order.
pub type ModelVariants = IndexMap<String, Map<String, Value>>;

/// The default model for a directory.
#[derive(Debug, Clone, PartialEq)]
pub struct DefaultModel {
    pub provider_id: String,
    pub model_id: String,
}

/// A directory snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub directory: String,
    pub providers: IndexMap<String, ProviderInfo>,
    pub model_options: Vec<ModelOption>,
    pub variants_by_model: IndexMap<String, ModelVariants>,
    pub available_modes: Vec<ModeOption>,
    pub default_mode_id: String,
    pub available_commands: Vec<CommandInfo>,
    pub default_model: Option<DefaultModel>,
}

/// Input to [`build`].
#[derive(Debug, Clone)]
pub struct BuildInput {
    pub directory: String,
    pub providers: IndexMap<String, ProviderInfo>,
    pub modes: Vec<ModeOption>,
    pub default_mode_id: String,
    pub commands: Vec<CommandInfo>,
    pub default_model: Option<DefaultModel>,
}

/// `modelKey` from reference/packages/opencode/src/acp/directory.ts.
pub fn model_key(model: &DefaultModel) -> String {
    format!("{}/{}", model.provider_id, model.model_id)
}

/// `variants` from reference/packages/opencode/src/acp/directory.ts.
pub fn variants<'a>(snapshot: &'a Snapshot, model: &DefaultModel) -> Option<&'a ModelVariants> {
    snapshot.variants_by_model.get(&model_key(model))
}

/// `build` from reference/packages/opencode/src/acp/directory.ts.
pub fn build(input: BuildInput) -> Snapshot {
    let mut model_options: Vec<ModelOption> = Vec::new();
    for provider in input.providers.values() {
        for model in provider.models.values() {
            model_options.push(ModelOption {
                provider_id: provider.id.clone(),
                provider_name: provider.name.clone(),
                model_id: model.id.clone(),
                model_name: model.name.clone(),
            });
        }
    }
    provider_sort(&mut model_options, |model| &model.model_id);

    let mut variants_by_model = IndexMap::new();
    for provider in input.providers.values() {
        for model in provider.models.values() {
            if let Some(model_variants) = &model.variants {
                variants_by_model.insert(
                    model_key(&DefaultModel {
                        provider_id: provider.id.clone(),
                        model_id: model.id.clone(),
                    }),
                    model_variants.clone(),
                );
            }
        }
    }

    let default_mode_id = if input
        .modes
        .iter()
        .any(|mode| mode.id == input.default_mode_id)
    {
        input.default_mode_id.clone()
    } else if let Some(first) = input.modes.first() {
        first.id.clone()
    } else {
        input.default_mode_id.clone()
    };

    Snapshot {
        directory: input.directory,
        providers: input.providers,
        model_options,
        variants_by_model,
        available_modes: input.modes,
        default_mode_id,
        available_commands: input.commands,
        default_model: input.default_model,
    }
}

/// `Provider.sort` from reference/packages/opencode/src/provider/provider.ts.
///
/// Sorts models by a priority filter list, preferring `latest` models first,
/// then by model id descending. Only the `model_id` is considered.
pub fn provider_sort<T>(models: &mut Vec<T>, key: impl for<'a> Fn(&'a T) -> &'a str) {
    const PRIORITY: [&str; 4] = ["gpt-5", "claude-sonnet-4", "big-pickle", "gemini-3-pro"];

    models.sort_by(|a, b| {
        let a_id = key(a);
        let b_id = key(b);
        let a_priority = PRIORITY
            .iter()
            .position(|filter| a_id.contains(filter))
            .map(|index| index as isize)
            .unwrap_or(-1);
        let b_priority = PRIORITY
            .iter()
            .position(|filter| b_id.contains(filter))
            .map(|index| index as isize)
            .unwrap_or(-1);
        b_priority.cmp(&a_priority).then_with(|| {
            let a_latest = if a_id.contains("latest") { 0 } else { 1 };
            let b_latest = if b_id.contains("latest") { 0 } else { 1 };
            a_latest.cmp(&b_latest).then_with(|| b_id.cmp(a_id))
        })
    });
}

/// A loader that produces snapshots for a directory.
#[async_trait::async_trait]
pub trait Loader: Send + Sync {
    /// `load` from reference/packages/opencode/src/acp/directory.ts.
    async fn load(&self, directory: &str) -> Result<Snapshot, ACPError>;
}

/// The caching directory service.
pub struct Service {
    loader: Box<dyn Loader>,
    snapshots: Mutex<HashMap<String, SharedSnapshot>>,
}

type SharedSnapshot = std::sync::Arc<tokio::sync::Mutex<Option<Result<Snapshot, ACPError>>>>;

impl Service {
    pub fn new(loader: Box<dyn Loader>) -> Self {
        Self {
            loader,
            snapshots: Mutex::new(HashMap::new()),
        }
    }

    /// `get` from reference/packages/opencode/src/acp/directory.ts. Concurrent
    /// callers share a single in-flight load; failures evict the cache entry.
    pub async fn get(&self, directory: &str) -> Result<Snapshot, ACPError> {
        let shared = self.shared_for(directory).await;
        if let Some(result) = shared.lock().await.clone() {
            return result;
        }
        let result = self.loader.load(directory).await;
        let mut slot = shared.lock().await;
        if slot.is_none() {
            if result.is_ok() {
                *slot = Some(result.clone());
            } else {
                drop(slot);
                self.snapshots.lock().await.remove(directory);
            }
            result
        } else {
            // Another task populated the cache while we were loading.
            slot.clone().unwrap_or(result)
        }
    }

    /// `refresh` from reference/packages/opencode/src/acp/directory.ts.
    pub async fn refresh(&self, directory: &str) -> Result<Snapshot, ACPError> {
        let shared = self.shared_for(directory).await;
        let result = self.loader.load(directory).await;
        *shared.lock().await = Some(result.clone());
        if result.is_err() {
            self.snapshots.lock().await.remove(directory);
        }
        result
    }

    async fn shared_for(&self, directory: &str) -> SharedSnapshot {
        let mut snapshots = self.snapshots.lock().await;
        snapshots.get(directory).cloned().unwrap_or_else(|| {
            let shared = std::sync::Arc::new(tokio::sync::Mutex::new(None));
            snapshots.insert(directory.to_string(), shared.clone());
            shared
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, models: &[(&str, &str, bool)]) -> ProviderInfo {
        ProviderInfo {
            id: id.into(),
            name: id.into(),
            source: "env".into(),
            env: vec![],
            key: None,
            options: Map::new(),
            models: models
                .iter()
                .map(|(model_id, name, has_variants)| {
                    let variants = if *has_variants {
                        Some(IndexMap::from_iter([("default".to_string(), Map::new())]))
                    } else {
                        None
                    };
                    (
                        model_id.to_string(),
                        crate::sdk::ModelInfo {
                            id: model_id.to_string(),
                            provider_id: id.into(),
                            name: name.to_string(),
                            variants,
                            limit: None,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn provider_sort_priority() {
        // Provider.sort uses `findIndex(...)` descending: later priority-list
        // entries sort first, unmatched models (index -1) sort last.
        let mut sorted = vec!["gemini-3-pro", "unknown-a", "gpt-5", "claude-sonnet-4"];
        provider_sort(&mut sorted, |model| model);
        assert_eq!(
            sorted,
            vec!["gemini-3-pro", "claude-sonnet-4", "gpt-5", "unknown-a"]
        );
    }

    #[test]
    fn build_snapshot() {
        let mut providers = IndexMap::new();
        providers.insert(
            "anthropic".into(),
            provider("anthropic", &[("claude-sonnet-4", "Claude", true)]),
        );
        providers.insert(
            "openai".into(),
            provider("openai", &[("gpt-5", "GPT-5", false)]),
        );
        let snapshot = build(BuildInput {
            directory: "/tmp".into(),
            providers,
            modes: vec![ModeOption {
                id: "build".into(),
                name: "build".into(),
                description: None,
            }],
            default_mode_id: "build".into(),
            commands: vec![],
            default_model: None,
        });
        assert_eq!(snapshot.default_mode_id, "build");
        let model = DefaultModel {
            provider_id: "anthropic".into(),
            model_id: "claude-sonnet-4".into(),
        };
        assert!(variants(&snapshot, &model).is_some());
        // priority sort: claude-sonnet-4 before gpt-5
        assert_eq!(snapshot.model_options[0].model_id, "claude-sonnet-4");
    }
}
