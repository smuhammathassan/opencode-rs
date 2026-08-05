//! OpenAI provider facade.
//! From reference/packages/llm/src/providers/openai.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::{openai_chat, openai_responses};
use crate::route::auth::{Auth, Credential};
use crate::route::auth_options as AuthOptions;
use crate::route::{EndpointPatch, Route, RoutePatch};
use crate::schema::{Model, ModelCompatibility, ModelDefaults};

use super::openai_options::with_openai_options;

pub const ID: &str = "openai";

/// `Config`.
/// From reference/packages/llm/src/providers/openai.ts (`Config`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub base_url: Option<String>,
    pub query_params: Option<BTreeMap<String, String>>,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub provider_options: Option<crate::schema::ProviderOptions>,
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<crate::schema::ModelLimits>,
    pub generation: Option<crate::schema::GenerationOptions>,
    pub http: Option<crate::schema::HttpOptions>,
}

fn auth_for(options: &Config) -> Auth {
    AuthOptions::bearer(options.auth.clone(), options.api_key.clone(), &["OPENAI_API_KEY"])
}

fn configure_route(route: &Route, options: &Config) -> Route {
    let mut patch = RoutePatch::empty();
    patch.auth = Some(auth_for(options));
    patch.endpoint = Some(EndpointPatch {
        base_url: options.base_url.clone(),
        path: None,
        query: options.query_params.clone(),
    });
    route.with(patch)
}

/// `OpenAI.configure(input)`.
/// From reference/packages/llm/src/providers/openai.ts (`configure`)
pub fn configure(input: Config) -> OpenAIProvider {
    let responses_route = configure_route(&openai_responses::route(), &input);
    let chat_route = configure_route(&openai_chat::route(), &input);
    let provider_options = input.provider_options.clone();

    let responses = move |model_id: String| -> Model {
        let options = with_openai_options(&model_id, provider_options.clone(), true);
        let route = responses_route.with(route_patch_with_options(options));
        route.model(crate::route::RouteModelInput { id: model_id, provider: None, defaults: None, compatibility: None }).unwrap()
    };
    let responses = Arc::new(responses);

    let chat = move |model_id: String| -> Model {
        let options = with_openai_options(&model_id, None, false);
        let route = chat_route.with(route_patch_with_options(options));
        route.model(crate::route::RouteModelInput { id: model_id, provider: None, defaults: None, compatibility: None }).unwrap()
    };
    let chat = Arc::new(chat);

    OpenAIProvider {
        id: ID.to_string(),
        responses,
        chat,
        configure: None,
    }
}

fn route_patch_with_options(options: Option<crate::schema::ProviderOptions>) -> RoutePatch {
    let mut patch = RoutePatch::empty();
    patch.provider_options = options;
    patch
}

/// Default `OpenAI` provider (env-key auth).
/// From reference/packages/llm/src/providers/openai.ts (`provider`)
pub fn provider() -> OpenAIProvider {
    configure(Config::default())
}

/// Provider handle.
#[derive(Clone)]
pub struct OpenAIProvider {
    pub id: String,
    pub responses: Arc<dyn Fn(String) -> Model + Send + Sync>,
    pub chat: Arc<dyn Fn(String) -> Model + Send + Sync>,
    pub configure: Option<fn(Config) -> OpenAIProvider>,
}

impl OpenAIProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.responses)(id.into())
    }
}

/// `OpenAI.routes`.
/// From reference/packages/llm/src/providers/openai.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![openai_responses::route(), openai_chat::route()]
}

#[allow(unused)]
fn _marker(_: &ModelDefaults, _: &ModelCompatibility, _: &Credential) {}
