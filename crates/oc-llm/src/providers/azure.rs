//! Azure OpenAI provider facade.
//! From reference/packages/llm/src/providers/azure.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::{openai_chat, openai_responses};
use crate::route::auth::{optional, Auth, Credential};
use crate::route::{EndpointPatch, Route, RouteDefaultsInput, RouteModelInput, RoutePatch};
use crate::schema::{GenerationOptions, HttpOptions, Model, ModelLimits, ProviderOptions};

pub const ID: &str = "azure";

/// `AzureURL` — `resourceName` or `baseURL` (at least one).
/// From reference/packages/llm/src/providers/azure.ts (`AzureURL`)
#[derive(Debug, Clone, Default)]
pub struct AzureUrl {
    pub resource_name: Option<String>,
    pub base_url: Option<String>,
}

/// `ModelOptions`.
/// From reference/packages/llm/src/providers/azure.ts (`ModelOptions`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub url: AzureUrl,
    pub api_version: Option<String>,
    pub query_params: Option<BTreeMap<String, String>>,
    pub use_completion_urls: bool,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub http: Option<HttpOptions>,
}

fn resource_base_url(resource_name: &str) -> String {
    format!("https://{}.openai.azure.com/openai/v1", resource_name.trim())
}

/// `routeAuth` — Azure removes `authorization`.
/// From reference/packages/llm/src/providers/azure.ts (`routeAuth`)
fn route_auth(input: &Config) -> Auth {
    if let Some(auth) = &input.auth {
        return auth.clone();
    }
    Auth::remove("authorization").and_then(
        optional(input.api_key.clone(), "apiKey")
            .or_else(Credential::Env("AZURE_OPENAI_API_KEY".to_string()))
            .header_auth("api-key"),
    )
}

fn responses_base_route() -> Route {
    let mut patch = RoutePatch::empty();
    patch.id = Some("azure-openai-responses".to_string());
    patch.provider = Some(ID.to_string());
    patch.auth = Some(Auth::remove("authorization"));
    patch.endpoint = Some(EndpointPatch::query(BTreeMap::from_iter([("api-version".to_string(), "v1".to_string())])));
    openai_responses::route().with(patch)
}

fn chat_base_route() -> Route {
    let mut patch = RoutePatch::empty();
    patch.id = Some("azure-openai-chat".to_string());
    patch.provider = Some(ID.to_string());
    patch.auth = Some(Auth::remove("authorization"));
    patch.endpoint = Some(EndpointPatch::query(BTreeMap::from_iter([("api-version".to_string(), "v1".to_string())])));
    openai_chat::route().with(patch)
}

fn configured_route(route: &Route, input: &Config) -> Route {
    let mut query = BTreeMap::new();
    if let Some(api_version) = &input.api_version {
        query.insert("api-version".to_string(), api_version.clone());
    }
    if let Some(query_params) = &input.query_params {
        query.extend(query_params.clone());
    }
    let base_url = input
        .url
        .base_url
        .clone()
        .or_else(|| input.url.resource_name.as_ref().map(|name| resource_base_url(name)));
    let mut patch = RoutePatch::empty();
    patch.auth = Some(route_auth(input));
    patch.endpoint = Some(EndpointPatch { base_url, path: None, query: Some(query) });
    patch.headers = input.headers.clone();
    patch.limits = input.limits.clone();
    patch.generation = input.generation.clone();
    patch.provider_options = input.provider_options.clone();
    patch.http = input.http.clone();
    route.with(patch)
}

/// `Azure.configure(input)`.
/// From reference/packages/llm/src/providers/azure.ts (`configure`)
pub fn configure(input: Config) -> AzureProvider {
    let responses_route = Arc::new(configured_route(&responses_base_route(), &input));
    let chat_route = Arc::new(configured_route(&chat_base_route(), &input));
    let use_completion_urls = input.use_completion_urls;
    let responses = {
        let route = responses_route.clone();
        move |model_id: String| -> Model {
            route.model(RouteModelInput { id: model_id, provider: None, defaults: None, compatibility: None }).unwrap()
        }
    };
    let chat = {
        let route = chat_route.clone();
        move |model_id: String| -> Model {
            route.model(RouteModelInput { id: model_id, provider: None, defaults: None, compatibility: None }).unwrap()
        }
    };
    AzureProvider {
        id: ID.to_string(),
        responses: Arc::new(responses),
        chat: Arc::new(chat),
        use_completion_urls,
    }
}

/// Provider handle.
#[derive(Clone)]
pub struct AzureProvider {
    pub id: String,
    pub responses: Arc<dyn Fn(String) -> Model + Send + Sync>,
    pub chat: Arc<dyn Fn(String) -> Model + Send + Sync>,
    pub use_completion_urls: bool,
}

impl AzureProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        let id = id.into();
        if self.use_completion_urls {
            (self.chat)(id)
        } else {
            (self.responses)(id)
        }
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/azure.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![responses_base_route(), chat_base_route()]
}

#[allow(unused)]
fn _marker(_: &RouteDefaultsInput) {}
