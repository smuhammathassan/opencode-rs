//! Provider/model catalog.
//!
//! From reference/packages/core/src/catalog.ts — `CatalogV2.Service`.
//! Backed by [`crate::state::State`]; publishes `catalog.updated` on reload.

use std::sync::Arc;

use indexmap::IndexMap;
use regex::Regex;
use serde_json::Value;

use crate::bus::{EventBus, PublishOptions};
use crate::event::Definition;
use crate::ids::{IntegrationId, ModelId, ProviderId};
use crate::integration::IntegrationService;
use crate::model::{ModelApi, ModelInfo};
use crate::policy::{Effect, PolicyService};
use crate::provider::{ProviderApi, ProviderInfo};
use crate::state::State;

/// `Catalog.ProviderRecord` — `{ provider, models }`.
#[derive(Debug, Clone)]
pub struct ProviderRecord {
    pub provider: ProviderInfo,
    pub models: IndexMap<ModelId, ModelInfo>,
}

/// `Catalog.DefaultModel` — `{ providerID, modelID }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultModel {
    pub provider_id: ProviderId,
    pub model_id: ModelId,
}

/// Catalog state (draft methods are inherent to this type).
#[derive(Debug, Clone, Default)]
pub struct CatalogData {
    pub providers: IndexMap<ProviderId, ProviderRecord>,
    pub default_model: Option<DefaultModel>,
}

impl CatalogData {
    pub fn provider_list(&self) -> Vec<ProviderRecord> {
        self.providers.values().cloned().collect()
    }

    pub fn provider_get(&self, provider_id: &ProviderId) -> Option<ProviderRecord> {
        self.providers.get(provider_id).cloned()
    }

    pub fn provider_update(&mut self, provider_id: &ProviderId, f: impl FnOnce(&mut ProviderInfo)) {
        let record = self
            .providers
            .entry(provider_id.clone())
            .or_insert_with(|| ProviderRecord {
                provider: ProviderInfo::empty(provider_id.clone()),
                models: IndexMap::new(),
            });
        f(&mut record.provider);
        normalize_api_provider(&mut record.provider);
    }

    pub fn provider_remove(&mut self, provider_id: &ProviderId) {
        self.providers.shift_remove(provider_id);
    }

    pub fn model_get(&self, provider_id: &ProviderId, model_id: &ModelId) -> Option<ModelInfo> {
        self.providers
            .get(provider_id)
            .and_then(|record| record.models.get(model_id))
            .cloned()
    }

    pub fn model_update(
        &mut self,
        provider_id: &ProviderId,
        model_id: &ModelId,
        f: impl FnOnce(&mut ModelInfo),
    ) {
        let record = self
            .providers
            .entry(provider_id.clone())
            .or_insert_with(|| ProviderRecord {
                provider: ProviderInfo::empty(provider_id.clone()),
                models: IndexMap::new(),
            });
        let model = record
            .models
            .entry(model_id.clone())
            .or_insert_with(|| ModelInfo::empty(provider_id.clone(), model_id.clone()));
        f(model);
        model.id = model_id.clone();
        model.providerID = provider_id.clone();
        normalize_api_model(model);
    }

    pub fn model_remove(&mut self, provider_id: &ProviderId, model_id: &ModelId) {
        if let Some(record) = self.providers.get_mut(provider_id) {
            record.models.shift_remove(model_id);
        }
    }

    pub fn model_default_get(&self) -> Option<DefaultModel> {
        self.default_model.clone()
    }

    pub fn model_default_set(&mut self, provider_id: ProviderId, model_id: ModelId) {
        self.default_model = Some(DefaultModel {
            provider_id,
            model_id,
        });
    }
}

fn normalize_api_provider(item: &mut ProviderInfo) {
    let Some(base_url) = item.request.body.get("baseURL").and_then(Value::as_str) else {
        return;
    };
    match &mut item.api {
        ProviderApi::Aisdk(api) => api.url = Some(base_url.to_string()),
        ProviderApi::Native(api) => api.url = Some(base_url.to_string()),
    }
    item.request.body.shift_remove("baseURL");
}

fn normalize_api_model(item: &mut ModelInfo) {
    let Some(base_url) = item.request.body.get("baseURL").and_then(Value::as_str) else {
        return;
    };
    item.api.url = Some(base_url.to_string());
    item.request.body.shift_remove("baseURL");
}

fn merge_settings(
    base: Option<&serde_json::Map<String, Value>>,
    overlay: Option<&serde_json::Map<String, Value>>,
) -> serde_json::Map<String, Value> {
    let mut merged = base.cloned().unwrap_or_default();
    if let Some(overlay) = overlay {
        for (key, value) in overlay {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

/// `projectModel(model, provider)` — fold provider api/request into the model.
fn project_model(model: &ModelInfo, provider: &ProviderInfo) -> ModelInfo {
    let api = if model.api.kind == "native"
        && model.api.url.is_none()
        && model
            .api
            .settings
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
    {
        match &provider.api {
            ProviderApi::Aisdk(api) => ModelApi {
                id: model.api.id.clone(),
                kind: "aisdk".to_string(),
                package: Some(api.package.clone()),
                url: api.url.clone(),
                settings: api.settings.clone(),
            },
            ProviderApi::Native(api) => ModelApi {
                id: model.api.id.clone(),
                kind: "native".to_string(),
                package: None,
                url: api.url.clone(),
                settings: Some(api.settings.clone()),
            },
        }
    } else if model.api.kind == "aisdk" && provider.api.is_aisdk() && model.api.url.is_none() {
        ModelApi {
            id: model.api.id.clone(),
            kind: "aisdk".to_string(),
            package: model.api.package.clone(),
            url: provider.api.url(),
            settings: Some(merge_settings(
                provider.api.settings(),
                model.api.settings.as_ref(),
            )),
        }
    } else if model.api.kind == "aisdk" && provider.api.is_aisdk() {
        ModelApi {
            id: model.api.id.clone(),
            kind: "aisdk".to_string(),
            package: model.api.package.clone(),
            url: model.api.url.clone(),
            settings: Some(merge_settings(
                provider.api.settings(),
                model.api.settings.as_ref(),
            )),
        }
    } else {
        model.api.clone()
    };
    let request = {
        let headers = merge_str_map(&provider.request.headers, &model.request.headers);
        let body = merge_settings(Some(&provider.request.body), Some(&model.request.body));
        crate::model::ModelRequest {
            headers,
            body,
            variant: model.request.variant.clone(),
        }
    };
    let mut result = model.clone();
    result.api = api;
    result.request = request;
    result
}

fn merge_str_map(
    base: &serde_json::Map<String, Value>,
    overlay: &serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let mut merged = base.clone();
    for (key, value) in overlay {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

impl ProviderApi {
    fn url(&self) -> Option<String> {
        match self {
            ProviderApi::Aisdk(api) => api.url.clone(),
            ProviderApi::Native(api) => api.url.clone(),
        }
    }

    fn settings(&self) -> Option<&serde_json::Map<String, Value>> {
        match self {
            ProviderApi::Aisdk(api) => api.settings.as_ref(),
            ProviderApi::Native(api) => Some(&api.settings),
        }
    }

    fn is_aisdk(&self) -> bool {
        matches!(self, ProviderApi::Aisdk(_))
    }
}

fn available(
    provider: &ProviderInfo,
    integration: Option<&crate::integration::IntegrationInfo>,
) -> bool {
    if provider.disabled == Some(true) {
        return false;
    }
    if matches!(provider.request.body.get("apiKey"), Some(Value::String(_))) {
        return true;
    }
    if integration
        .map(|i| !i.connections.is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    provider.integrationID.is_none() && integration.is_none()
}

/// The catalog service (`@opencode/v2/Catalog`).
#[derive(Clone)]
pub struct CatalogService {
    state: Arc<State<CatalogData>>,
    integrations: Arc<IntegrationService>,
}

pub const CATALOG_UPDATED: &str = "catalog.updated";

impl CatalogService {
    pub fn new(
        bus: Arc<EventBus>,
        policy: Arc<PolicyService>,
        integrations: Arc<IntegrationService>,
    ) -> Self {
        let definition = Definition::define(CATALOG_UPDATED, None, vec![]);
        let bus_clone = bus.clone();
        let policy_clone = policy.clone();
        let finalize: Option<crate::state::TransformCallback<CatalogData>> = {
            let definition = definition.clone();
            Some(Arc::new(move |data: &mut CatalogData| {
                let definition = definition.clone();
                let bus = bus_clone.clone();
                let policy = policy_clone.clone();
                Box::pin(async move {
                    if policy.has_statements() {
                        let records: Vec<ProviderRecord> = data.provider_list();
                        for record in records {
                            let effect = policy
                                .evaluate("provider.use", &record.provider.id.0, Effect::Allow)
                                .await;
                            if effect == Effect::Deny {
                                data.provider_remove(&record.provider.id);
                            }
                        }
                    }
                    let _ = bus
                        .publish(
                            &definition,
                            &serde_json::Map::new(),
                            &PublishOptions::default(),
                        )
                        .await
                        .map_err(|e| e.to_string());
                    Ok(())
                })
            }))
        };
        CatalogService {
            state: Arc::new(State::create(CatalogData::default(), finalize)),
            integrations,
        }
    }

    pub fn state(&self) -> &Arc<State<CatalogData>> {
        &self.state
    }

    pub async fn provider_get(&self, provider_id: &ProviderId) -> Option<ProviderInfo> {
        self.state
            .get()
            .providers
            .get(provider_id)
            .map(|record| record.provider.clone())
    }

    pub async fn provider_all(&self) -> Vec<ProviderInfo> {
        self.state
            .get()
            .providers
            .values()
            .map(|record| record.provider.clone())
            .collect()
    }

    pub async fn provider_available(&self) -> Vec<ProviderInfo> {
        let active: IndexMap<IntegrationId, crate::integration::IntegrationInfo> = self
            .integrations
            .list()
            .await
            .into_iter()
            .map(|integration| (integration.id.clone(), integration))
            .collect();
        self.provider_all()
            .await
            .into_iter()
            .filter(|provider| {
                let key = provider
                    .integrationID
                    .clone()
                    .unwrap_or_else(|| IntegrationId(provider.id.0.clone()));
                available(provider, active.get(&key))
            })
            .collect()
    }

    pub async fn model_get(
        &self,
        provider_id: &ProviderId,
        model_id: &ModelId,
    ) -> Option<ModelInfo> {
        let data = self.state.get();
        let record = data.providers.get(provider_id)?;
        record
            .models
            .get(model_id)
            .map(|model| project_model(model, &record.provider))
    }

    pub async fn model_all(&self) -> Vec<ModelInfo> {
        let mut models: Vec<ModelInfo> = self
            .state
            .get()
            .providers
            .values()
            .flat_map(|record| {
                record
                    .models
                    .values()
                    .map(|model| project_model(model, &record.provider))
            })
            .collect();
        models.sort_by(|a, b| {
            b.time
                .released
                .partial_cmp(&a.time.released)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        models
    }

    pub async fn model_available(&self) -> Vec<ModelInfo> {
        let providers: std::collections::HashSet<ProviderId> = self
            .provider_available()
            .await
            .into_iter()
            .map(|provider| provider.id)
            .collect();
        self.model_all()
            .await
            .into_iter()
            .filter(|model| providers.contains(&model.providerID) && model.enabled)
            .collect()
    }

    pub async fn model_default(&self) -> Option<ModelInfo> {
        if let Some(default) = &self.state.get().default_model {
            let provider = self.provider_get(&default.provider_id).await;
            if let Some(provider) = provider {
                let available = self.provider_available().await;
                if available.iter().any(|item| item.id == provider.id) {
                    let model = self
                        .model_get(&default.provider_id, &default.model_id)
                        .await;
                    if let Some(model) = model {
                        if model.enabled {
                            return Some(model);
                        }
                    }
                }
            }
        }
        let mut available = self.model_available().await;
        available.sort_by(|a, b| {
            b.time
                .released
                .partial_cmp(&a.time.released)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        available.into_iter().next()
    }

    pub async fn model_small(&self, provider_id: &ProviderId) -> Option<ModelInfo> {
        let data = self.state.get();
        let record = data.providers.get(provider_id)?;
        let provider = &record.provider;

        if provider_id == &ProviderId::azure()
            || provider_id == &ProviderId::make("azure-cognitive-services")
        {
            return None;
        }

        if provider_id == &ProviderId::opencode() {
            let gpt5_nano = record.models.get(&ModelId::make("gpt-5-nano"));
            if let Some(model) = gpt5_nano {
                if model.enabled && model.status == "active" {
                    return Some(project_model(model, provider));
                }
            }
        }

        let mut candidates: Vec<SmallCandidate> = record
            .models
            .values()
            .filter(|model| {
                model.providerID == *provider_id
                    && model.enabled
                    && model.status == "active"
                    && model
                        .capabilities
                        .input
                        .iter()
                        .any(|item| item.starts_with("text"))
                    && model
                        .capabilities
                        .output
                        .iter()
                        .any(|item| item.starts_with("text"))
            })
            .map(|model| SmallCandidate {
                model: model.clone(),
                cost: model
                    .cost
                    .first()
                    .map(|cost| cost.input + cost.output)
                    .unwrap_or(999.0),
                age: (now_ms() - model.time.released) / (1000.0 * 60.0 * 60.0 * 24.0 * 30.0),
                small: SMALL_MODEL_RE.is_match(
                    &format!(
                        "{} {} {}",
                        model.id.0,
                        model.family.clone().unwrap_or_default(),
                        model.name
                    )
                    .to_lowercase(),
                ),
            })
            .filter(|item| item.cost > 0.0 && item.age <= 18.0)
            .collect();

        let pick = |items: &mut Vec<SmallCandidate>| -> Option<ModelInfo> {
            let max_cost = items
                .iter()
                .map(|item| item.cost)
                .fold(f64::MIN, f64::max)
                .max(0.01);
            let max_age = items
                .iter()
                .map(|item| item.age)
                .fold(f64::MIN, f64::max)
                .max(0.01);
            items.sort_by(|a, b| {
                let a_score = (a.cost / max_cost) * 0.8 + (a.age / max_age) * 0.2;
                let b_score = (b.cost / max_cost) * 0.8 + (b.age / max_age) * 0.2;
                a_score
                    .partial_cmp(&b_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            items
                .first()
                .map(|item| project_model(&item.model, provider))
        };

        let mut small_items: Vec<SmallCandidate> = candidates
            .iter()
            .filter(|item| item.small)
            .cloned()
            .collect();
        if !small_items.is_empty() {
            pick(&mut small_items)
        } else {
            pick(&mut candidates)
        }
    }
}

#[derive(Debug, Clone)]
struct SmallCandidate {
    model: ModelInfo,
    cost: f64,
    age: f64,
    small: bool,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

// `\b(nano|flash|lite|mini|haiku|small|fast)\b` — input is lowercased first.
static SMALL_MODEL_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"\b(nano|flash|lite|mini|haiku|small|fast)\b").expect("valid regex")
});

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> CatalogService {
        CatalogService::new(
            Arc::new(EventBus::in_memory()),
            Arc::new(PolicyService::new()),
            Arc::new(IntegrationService::new()),
        )
    }

    #[tokio::test]
    async fn provider_available_filters_disabled() {
        let catalog = service();
        let data = catalog.state().get();
        let mut next = data;
        next.provider_update(&ProviderId::make("p1"), |provider| {
            provider.name = "P1".to_string();
        });
        next.provider_update(&ProviderId::make("p2"), |provider| {
            provider.name = "P2".to_string();
            provider.disabled = Some(true);
        });
        next.provider_update(&ProviderId::make("p3"), |provider| {
            provider.name = "P3".to_string();
            provider
                .request
                .body
                .insert("apiKey".to_string(), Value::from("x"));
        });
        // All three are available: p1 (no integration), p2 is disabled.
        let records = next.provider_list();
        let p1 = records
            .iter()
            .find(|r| r.provider.id == ProviderId::make("p1"))
            .unwrap();
        let p2 = records
            .iter()
            .find(|r| r.provider.id == ProviderId::make("p2"))
            .unwrap();
        assert!(available(&p1.provider, None));
        assert!(!available(&p2.provider, None));
        let p3 = records
            .iter()
            .find(|r| r.provider.id == ProviderId::make("p3"))
            .unwrap();
        assert!(available(&p3.provider, None));
    }

    #[tokio::test]
    async fn project_model_native_inherits_provider_url() {
        let catalog = service();
        let data = catalog.state().get();
        let mut next = data;
        next.provider_update(&ProviderId::make("acme"), |provider| {
            provider.name = "Acme".to_string();
        });
        next.model_update(
            &ProviderId::make("acme"),
            &ModelId::make("model-a"),
            |model| {
                model.name = "Model A".to_string();
            },
        );
        let provider = next
            .provider_get(&ProviderId::make("acme"))
            .unwrap()
            .provider;
        let model = next
            .model_get(&ProviderId::make("acme"), &ModelId::make("model-a"))
            .unwrap();
        let projected = project_model(&model, &provider);
        assert_eq!(projected.api.kind, "native");
        assert_eq!(projected.api.id, ModelId::make("model-a"));
    }

    #[test]
    fn small_model_regex() {
        assert!(SMALL_MODEL_RE.is_match("gpt-4o-mini"));
        assert!(SMALL_MODEL_RE.is_match("claude-3-5-haiku"));
        assert!(!SMALL_MODEL_RE.is_match("gpt-4o"));
    }
}
