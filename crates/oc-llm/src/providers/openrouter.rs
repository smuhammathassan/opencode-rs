//! OpenRouter provider facade.
//! From reference/packages/llm/src/providers/openrouter.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::protocols::openai_chat;
use crate::providers::openai_compatible_profile;
use crate::route::auth::Auth;
use crate::route::auth_options as AuthOptions;
use crate::route::{EndpointPatch, Route, RouteDefaultsInput, RouteModelInput, RoutePatch};
use crate::schema::{GenerationOptions, HttpOptions, Model, ModelLimits, ProviderOptions};
use crate::shared::is_record;

pub const ID: &str = "openrouter";
pub const ADAPTER: &str = "openrouter";

/// `OpenRouterProviderOptionsInput`.
/// From reference/packages/llm/src/providers/openrouter.ts
pub type OpenRouterOptions = BTreeMap<String, Value>;

/// `ModelOptions`.
/// From reference/packages/llm/src/providers/openrouter.ts (`ModelOptions`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub http: Option<HttpOptions>,
}

/// `OpenRouter.bodyOptions(input)` — maps provider options onto the body.
/// From reference/packages/llm/src/providers/openrouter.ts (`bodyOptions`)
pub fn body_options(openrouter: Option<&OpenRouterOptions>) -> serde_json::Map<String, Value> {
    let mut result = serde_json::Map::new();
    let Some(openrouter) = openrouter else {
        return result;
    };
    let usage = openrouter.get("usage");
    if usage == Some(&Value::Bool(true)) {
        result.insert(
            "usage".to_string(),
            Value::Object(serde_json::Map::from_iter([(
                "include".to_string(),
                Value::Bool(true),
            )])),
        );
    } else if is_record(usage.unwrap_or(&Value::Null)) {
        result.insert("usage".to_string(), usage.cloned().unwrap_or(Value::Null));
    }
    if let Some(reasoning) = openrouter.get("reasoning") {
        if is_record(reasoning) {
            result.insert("reasoning".to_string(), reasoning.clone());
        }
    }
    if let Some(prompt_cache_key) = openrouter.get("promptCacheKey").and_then(Value::as_str) {
        result.insert(
            "prompt_cache_key".to_string(),
            Value::String(prompt_cache_key.to_string()),
        );
    }
    result
}

/// `OpenRouter.fromRequest` — OpenAIChat body plus openrouter options.
/// From reference/packages/llm/src/providers/openrouter.ts
pub fn from_request(request: &crate::schema::LlmRequest) -> Result<Value, crate::schema::LlmError> {
    let mut body = openai_chat::from_request(request)?;
    let openrouter = request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("openrouter"));
    if let Value::Object(obj) = &mut body {
        for (key, value) in body_options(openrouter) {
            obj.insert(key, value);
        }
    }
    Ok(body)
}

/// `route`.
/// From reference/packages/llm/src/providers/openrouter.ts (`route`)
pub fn route() -> Route {
    let profile = openai_compatible_profile::by_provider("openrouter").unwrap();
    Route::make(crate::route::RouteMakeInput {
        id: ADAPTER.to_string(),
        provider: Some(profile.provider.to_string()),
        protocol: crate::route::Protocol::make(
            "openrouter-chat",
            Arc::new(|request| from_request(request)),
            openai_chat::protocol().stream,
        ),
        endpoint: crate::route::endpoint::path(
            "/chat/completions",
            crate::route::EndpointOptions {
                base_url: Some(profile.base_url.to_string()),
                query: None,
            },
        ),
        auth: None,
        framing: Some(crate::route::Framing::Sse),
        headers: None,
        defaults: None,
    })
}

/// `configure(input)`.
/// From reference/packages/llm/src/providers/openrouter.ts (`configure`)
pub fn configure(input: Config) -> OpenRouterProvider {
    let profile = openai_compatible_profile::by_provider("openrouter").unwrap();
    let mut patch = RoutePatch::empty();
    patch.auth = Some(AuthOptions::bearer(
        input.auth.clone(),
        input.api_key.clone(),
        &["OPENROUTER_API_KEY"],
    ));
    patch.endpoint = Some(EndpointPatch::base_url(
        input
            .base_url
            .clone()
            .unwrap_or_else(|| profile.base_url.to_string()),
    ));
    patch.headers = input.headers.clone();
    patch.limits = input.limits.clone();
    patch.generation = input.generation.clone();
    patch.provider_options = input.provider_options.clone();
    patch.http = input.http.clone();
    let route = Arc::new(route().with(patch));
    let model = move |model_id: String| -> Model {
        route
            .model(RouteModelInput {
                id: model_id,
                provider: Some(ID.to_string()),
                defaults: None,
                compatibility: None,
            })
            .unwrap()
    };
    OpenRouterProvider {
        id: ID.to_string(),
        model: Arc::new(model),
    }
}

/// Default provider (env-key auth).
/// From reference/packages/llm/src/providers/openrouter.ts (`provider`)
pub fn provider() -> OpenRouterProvider {
    configure(Config::default())
}

/// Provider handle.
#[derive(Clone)]
pub struct OpenRouterProvider {
    pub id: String,
    pub model: Arc<dyn Fn(String) -> Model + Send + Sync>,
}

impl OpenRouterProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.model)(id.into())
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/openrouter.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![route()]
}

#[allow(unused)]
fn _marker(_: &RouteDefaultsInput) {}
