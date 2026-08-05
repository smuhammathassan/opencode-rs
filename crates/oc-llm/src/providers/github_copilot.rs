//! GitHub Copilot provider facade.
//! From reference/packages/llm/src/providers/github-copilot.ts

use std::sync::Arc;

use crate::protocols::{openai_chat, openai_responses};
use crate::route::auth::Auth;
use crate::route::auth_options as AuthOptions;
use crate::route::{EndpointPatch, Route, RouteModelInput, RoutePatch};
use crate::schema::Model;

pub const ID: &str = "github-copilot";

/// `ModelOptions`.
/// From reference/packages/llm/src/providers/github-copilot.ts (`ModelOptions`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub base_url: String,
    pub endpoint: Option<EndpointKind>,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub provider_options: Option<crate::schema::ProviderOptions>,
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    pub limits: Option<crate::schema::ModelLimits>,
    pub generation: Option<crate::schema::GenerationOptions>,
    pub http: Option<crate::schema::HttpOptions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointKind {
    Chat,
    Responses,
}

/// `shouldUseResponsesApi(modelID, endpoint)`.
/// From reference/packages/llm/src/providers/github-copilot.ts (`shouldUseResponsesApi`)
pub fn should_use_responses_api(model_id: &str, endpoint: Option<EndpointKind>) -> bool {
    if let Some(endpoint) = endpoint {
        return endpoint == EndpointKind::Responses;
    }
    let model = model_id;
    let Some(rest) = model.strip_prefix("gpt-") else {
        return false;
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let Ok(major) = digits.parse::<u32>() else {
        return false;
    };
    major >= 5 && !model.starts_with("gpt-5-mini")
}

fn chat_base_route() -> Route {
    let mut patch = RoutePatch::empty();
    patch.provider = Some(ID.to_string());
    openai_chat::route().with(patch)
}

fn responses_base_route() -> Route {
    let mut patch = RoutePatch::empty();
    patch.provider = Some(ID.to_string());
    openai_responses::route().with(patch)
}

/// `configure(options)`.
/// From reference/packages/llm/src/providers/github-copilot.ts (`configure`)
pub fn configure(input: Config) -> GitHubCopilotProvider {
    let auth = AuthOptions::bearer(input.auth.clone(), input.api_key.clone(), &[]);
    let chat_route = chat_base_route().with(RoutePatch {
        auth: Some(auth.clone()),
        endpoint: Some(EndpointPatch::base_url(input.base_url.clone())),
        headers: input.headers.clone(),
        limits: input.limits.clone(),
        generation: input.generation.clone(),
        provider_options: input.provider_options.clone(),
        http: input.http.clone(),
        ..Default::default()
    });
    let responses_route = responses_base_route().with(RoutePatch {
        auth: Some(auth),
        endpoint: Some(EndpointPatch::base_url(input.base_url.clone())),
        headers: input.headers.clone(),
        limits: input.limits.clone(),
        generation: input.generation.clone(),
        provider_options: input.provider_options.clone(),
        http: input.http.clone(),
        ..Default::default()
    });
    let endpoint = input.endpoint;
    let chat_route = Arc::new(chat_route);
    let responses_route = Arc::new(responses_route);
    let chat = {
        let route = chat_route.clone();
        move |model_id: String| -> Model {
            route
                .model(RouteModelInput {
                    id: model_id,
                    provider: None,
                    defaults: None,
                    compatibility: None,
                })
                .unwrap()
        }
    };
    let responses = {
        let route = responses_route.clone();
        move |model_id: String| -> Model {
            route
                .model(RouteModelInput {
                    id: model_id,
                    provider: None,
                    defaults: None,
                    compatibility: None,
                })
                .unwrap()
        }
    };
    GitHubCopilotProvider {
        id: ID.to_string(),
        chat: Arc::new(chat),
        responses: Arc::new(responses),
        endpoint,
    }
}

/// Provider handle.
#[derive(Clone)]
pub struct GitHubCopilotProvider {
    pub id: String,
    pub chat: Arc<dyn Fn(String) -> Model + Send + Sync>,
    pub responses: Arc<dyn Fn(String) -> Model + Send + Sync>,
    pub endpoint: Option<EndpointKind>,
}

impl GitHubCopilotProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        let id = id.into();
        if should_use_responses_api(&id, self.endpoint) {
            (self.responses)(id)
        } else {
            (self.chat)(id)
        }
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/github-copilot.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![responses_base_route(), chat_base_route()]
}

/// `provider` — requires explicit `baseURL`.
/// From reference/packages/llm/src/providers/github-copilot.ts (`provider`)
pub fn provider(base_url: String) -> GitHubCopilotProvider {
    configure(Config {
        base_url,
        ..Default::default()
    })
}
