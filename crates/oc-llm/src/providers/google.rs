//! Google Gemini provider facade.
//! From reference/packages/llm/src/providers/google.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::gemini;
use crate::route::auth::{optional, Auth, Credential};
use crate::route::{EndpointPatch, Route, RouteModelInput, RoutePatch};
use crate::schema::{GenerationOptions, HttpOptions, Model, ModelLimits, ProviderOptions};

pub const ID: &str = "google";

/// `Config`.
/// From reference/packages/llm/src/providers/google.ts (`Config`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

fn auth_for(options: &Config) -> Auth {
    if let Some(auth) = &options.auth {
        return auth.clone();
    }
    optional(options.api_key.clone(), "apiKey")
        .or_else(Credential::Env("GOOGLE_GENERATIVE_AI_API_KEY".to_string()))
        .header_auth("x-goog-api-key")
}

/// `Google.configure(input)`.
/// From reference/packages/llm/src/providers/google.ts (`configure`)
pub fn configure(input: Config) -> GoogleProvider {
    let mut patch = RoutePatch::empty();
    patch.auth = Some(auth_for(&input));
    patch.endpoint = Some(EndpointPatch::base_url(input.base_url.clone().unwrap_or_else(|| gemini::DEFAULT_BASE_URL.to_string())));
    patch.headers = input.headers.clone();
    patch.limits = input.limits.clone();
    patch.generation = input.generation.clone();
    patch.provider_options = input.provider_options.clone();
    patch.http = input.http.clone();
    let route = Arc::new(gemini::route().with(patch));
    let model = move |model_id: String| -> Model {
        route.model(RouteModelInput { id: model_id, provider: None, defaults: None, compatibility: None }).unwrap()
    };
    GoogleProvider { id: ID.to_string(), model: Arc::new(model) }
}

/// Default provider (env-key auth).
/// From reference/packages/llm/src/providers/google.ts (`provider`)
pub fn provider() -> GoogleProvider {
    configure(Config::default())
}

/// Provider handle.
#[derive(Clone)]
pub struct GoogleProvider {
    pub id: String,
    pub model: Arc<dyn Fn(String) -> Model + Send + Sync>,
}

impl GoogleProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.model)(id.into())
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/google.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![gemini::route()]
}
