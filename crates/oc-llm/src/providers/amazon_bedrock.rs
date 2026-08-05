//! Amazon Bedrock provider facade.
//! From reference/packages/llm/src/providers/amazon-bedrock.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::bedrock_converse;
use crate::route::auth::{Auth, Credential};
use crate::route::{EndpointPatch, Route, RouteModelInput, RoutePatch};
use crate::schema::{GenerationOptions, HttpOptions, Model, ModelLimits, ProviderOptions};

pub const ID: &str = "amazon-bedrock";

/// `BedrockCredentials`.
/// From reference/packages/llm/src/protocols/utils/bedrock-auth.ts (`Credentials`)
#[derive(Debug, Clone, Default)]
pub struct BedrockCredentials {
    pub region: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
}

/// `Config`.
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`Config`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub api_key: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub credentials: Option<BedrockCredentials>,
    pub region: Option<String>,
    pub base_url: Option<String>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

fn bedrock_base_url(region: &str) -> String {
    format!("https://bedrock-runtime.{}.amazonaws.com", region)
}

/// SigV4 route auth. AWS SigV4 request signing is not implemented in this
/// port (`TODO(integration)`): requests fall back to bearer auth when an API
/// key is configured, otherwise they fail with a clear error.
/// From reference/packages/llm/src/protocols/utils/bedrock-auth.ts (`sigV4`)
pub fn sig_v4_auth(_credentials: Option<&BedrockCredentials>) -> Auth {
    crate::route::auth::custom(|_input| {
        Err(crate::shared::invalid_request(
            "Bedrock Converse requires either route bearer auth or AWS credentials configured on the route",
        ))
    })
}

/// `AmazonBedrock.configure(input)`.
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`configure`)
pub fn configure(input: Config) -> BedrockProvider {
    let resolved_region = input
        .region
        .clone()
        .or_else(|| input.credentials.as_ref().and_then(|c| c.region.clone()))
        .unwrap_or_else(|| "us-east-1".to_string());
    let mut patch = RoutePatch::empty();
    patch.provider = Some(ID.to_string());
    patch.endpoint = Some(EndpointPatch::base_url(
        input.base_url.clone().unwrap_or_else(|| bedrock_base_url(&resolved_region)),
    ));
    patch.auth = Some(match &input.api_key {
        Some(api_key) => Credential::Value(api_key.clone()).bearer_auth(),
        None => sig_v4_auth(input.credentials.as_ref()),
    });
    patch.headers = input.headers.clone();
    patch.limits = input.limits.clone();
    patch.generation = input.generation.clone();
    patch.provider_options = input.provider_options.clone();
    patch.http = input.http.clone();
    let route = Arc::new(bedrock_converse::route().with(patch));
    let model = move |model_id: String| -> Model {
        route.model(RouteModelInput { id: model_id, provider: Some(ID.to_string()), defaults: None, compatibility: None }).unwrap()
    };
    BedrockProvider { id: ID.to_string(), model: Arc::new(model) }
}

/// Default provider (env-based credentials).
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`provider`)
pub fn provider() -> BedrockProvider {
    configure(Config::default())
}

/// Provider handle.
#[derive(Clone)]
pub struct BedrockProvider {
    pub id: String,
    pub model: Arc<dyn Fn(String) -> Model + Send + Sync>,
}

impl BedrockProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.model)(id.into())
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![bedrock_converse::route()]
}
