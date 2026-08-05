//! OpenAI-compatible provider facade.
//! From reference/packages/llm/src/providers/openai-compatible.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::openai_compatible_chat;
use crate::route::auth::Auth;
use crate::route::auth_options as AuthOptions;
use crate::route::{EndpointPatch, Route, RouteDefaultsInput, RouteModelInput, RoutePatch};
use crate::schema::{GenerationOptions, HttpOptions, Model, ModelLimits, ProviderOptions};

pub const ID: &str = "openai-compatible";

/// `GenericModelOptions`.
/// From reference/packages/llm/src/providers/openai-compatible.ts
#[derive(Debug, Clone, Default)]
pub struct GenericModelOptions {
    pub provider: Option<String>,
    pub base_url: String,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

/// `OpenAICompatible` provider facade.
/// From reference/packages/llm/src/providers/openai-compatible.ts (`configure`)
pub struct OpenAICompatible {
    pub id: String,
    pub model: Arc<dyn Fn(String) -> Model + Send + Sync>,
}

impl OpenAICompatible {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.model)(id.into())
    }
}

#[allow(dead_code)]
fn defaults_patch(options: &GenericModelOptions) -> RouteDefaultsInput {
    RouteDefaultsInput {
        headers: options.headers.clone(),
        limits: options.limits.clone(),
        generation: options.generation.clone(),
        provider_options: options.provider_options.clone(),
        http: options.http.clone(),
    }
}

/// `configure(input)`.
/// From reference/packages/llm/src/providers/openai-compatible.ts (`configure`)
pub fn configure(input: GenericModelOptions) -> OpenAICompatible {
    let provider = input
        .provider
        .clone()
        .unwrap_or_else(|| "openai-compatible".to_string());
    let route = openai_compatible_chat::route().with(RoutePatch {
        id: None,
        provider: Some(provider.clone()),
        auth: Some(AuthOptions::bearer(
            input.auth.clone(),
            input.api_key.clone(),
            &[],
        )),
        endpoint: Some(EndpointPatch::base_url(input.base_url.clone())),
        headers: None,
        headers_fn: None,
        limits: input.limits.clone(),
        generation: input.generation.clone(),
        provider_options: input.provider_options.clone(),
        http: input.http.clone(),
    });
    let route = Arc::new(route);
    let id_for_closure = provider.clone();
    let model = move |model_id: String| -> Model {
        route
            .model(RouteModelInput {
                id: model_id,
                provider: Some(id_for_closure.clone()),
                defaults: None,
                compatibility: None,
            })
            .unwrap()
    };
    OpenAICompatible {
        id: provider,
        model: Arc::new(model),
    }
}

/// `provider()` — the default compatible provider with no URL.
/// From reference/packages/llm/src/providers/openai-compatible.ts (`provider`)
pub fn provider() -> OpenAICompatible {
    configure(GenericModelOptions {
        base_url: String::new(),
        ..Default::default()
    })
}

/// Define a family facade from a profile.
/// From reference/packages/llm/src/providers/openai-compatible.ts (`define`)
pub fn define(
    profile: crate::providers::openai_compatible_profile::OpenAICompatibleProfile,
) -> OpenAICompatible {
    configure(GenericModelOptions {
        provider: Some(profile.provider.to_string()),
        base_url: profile.base_url.to_string(),
        ..Default::default()
    })
}

pub fn baseten() -> OpenAICompatible {
    define(crate::providers::openai_compatible_profile::by_provider("baseten").unwrap())
}

pub fn cerebras() -> OpenAICompatible {
    define(crate::providers::openai_compatible_profile::by_provider("cerebras").unwrap())
}

pub fn deepinfra() -> OpenAICompatible {
    define(crate::providers::openai_compatible_profile::by_provider("deepinfra").unwrap())
}

pub fn deepseek() -> OpenAICompatible {
    define(crate::providers::openai_compatible_profile::by_provider("deepseek").unwrap())
}

pub fn fireworks() -> OpenAICompatible {
    define(crate::providers::openai_compatible_profile::by_provider("fireworks").unwrap())
}

pub fn groq() -> OpenAICompatible {
    define(crate::providers::openai_compatible_profile::by_provider("groq").unwrap())
}

pub fn togetherai() -> OpenAICompatible {
    define(crate::providers::openai_compatible_profile::by_provider("togetherai").unwrap())
}

/// `routes`.
/// From reference/packages/llm/src/providers/openai-compatible.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![openai_compatible_chat::route()]
}

#[allow(unused)]
fn _defaults_marker(_: &RouteDefaultsInput) {}
