//! XAI provider facade.
//! From reference/packages/llm/src/providers/xai.ts

use std::sync::Arc;

use crate::protocols::{openai_compatible_chat, openai_responses};
use crate::providers::openai_compatible_profile;
use crate::route::auth::Auth;
use crate::route::auth_options as AuthOptions;
use crate::route::{EndpointPatch, Route, RouteModelInput, RoutePatch};
use crate::schema::Model;

pub const ID: &str = "xai";

/// `ModelOptions`.
/// From reference/packages/llm/src/providers/xai.ts (`ModelOptions`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub provider_options: Option<crate::schema::ProviderOptions>,
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    pub limits: Option<crate::schema::ModelLimits>,
    pub generation: Option<crate::schema::GenerationOptions>,
    pub http: Option<crate::schema::HttpOptions>,
}

fn auth_for(options: &Config) -> Auth {
    AuthOptions::bearer(
        options.auth.clone(),
        options.api_key.clone(),
        &["XAI_API_KEY"],
    )
}

fn configured_responses_route(input: &Config) -> Route {
    let profile = openai_compatible_profile::by_provider("xai").unwrap();
    let mut patch = RoutePatch::empty();
    patch.provider = Some(ID.to_string());
    patch.auth = Some(auth_for(input));
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
    openai_responses::route().with(patch)
}

fn configured_chat_route(input: &Config) -> Route {
    let profile = openai_compatible_profile::by_provider("xai").unwrap();
    let mut patch = RoutePatch::empty();
    patch.provider = Some(ID.to_string());
    patch.auth = Some(auth_for(input));
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
    openai_compatible_chat::route().with(patch)
}

/// `configure(input)`.
/// From reference/packages/llm/src/providers/xai.ts (`configure`)
pub fn configure(input: Config) -> XaiProvider {
    let responses_route = Arc::new(configured_responses_route(&input));
    let chat_route = Arc::new(configured_chat_route(&input));
    let responses = {
        let route = responses_route.clone();
        move |model_id: String| -> Model {
            route
                .model(RouteModelInput {
                    id: model_id,
                    provider: Some(ID.to_string()),
                    defaults: None,
                    compatibility: None,
                })
                .unwrap()
        }
    };
    let chat = {
        let route = chat_route.clone();
        move |model_id: String| -> Model {
            route
                .model(RouteModelInput {
                    id: model_id,
                    provider: Some(ID.to_string()),
                    defaults: None,
                    compatibility: None,
                })
                .unwrap()
        }
    };
    XaiProvider {
        id: ID.to_string(),
        responses: Arc::new(responses),
        chat: Arc::new(chat),
    }
}

/// Default provider (env-key auth).
/// From reference/packages/llm/src/providers/xai.ts (`provider`)
pub fn provider() -> XaiProvider {
    configure(Config::default())
}

/// Provider handle.
#[derive(Clone)]
pub struct XaiProvider {
    pub id: String,
    pub responses: Arc<dyn Fn(String) -> Model + Send + Sync>,
    pub chat: Arc<dyn Fn(String) -> Model + Send + Sync>,
}

impl XaiProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.responses)(id.into())
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/xai.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![openai_responses::route(), openai_compatible_chat::route()]
}
