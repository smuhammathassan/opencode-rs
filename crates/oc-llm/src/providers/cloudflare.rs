//! Cloudflare AI Gateway and Workers AI facades.
//! From reference/packages/llm/src/providers/cloudflare.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::openai_compatible_chat;
use crate::route::auth::{Auth, Credential};
use crate::route::auth_options as AuthOptions;
use crate::route::{EndpointPatch, Route, RouteModelInput, RoutePatch};
use crate::schema::{GenerationOptions, HttpOptions, Model, ModelLimits, ProviderOptions};

pub const AI_GATEWAY_ID: &str = "cloudflare-ai-gateway";
pub const WORKERS_AI_ID: &str = "cloudflare-workers-ai";
pub const AI_GATEWAY_AUTH_ENV_VARS: [&str; 2] = ["CLOUDFLARE_API_TOKEN", "CF_AIG_TOKEN"];
pub const WORKERS_AI_AUTH_ENV_VARS: [&str; 2] = ["CLOUDFLARE_API_KEY", "CLOUDFLARE_WORKERS_AI_TOKEN"];

/// `GatewayURL` — `accountId` or `baseURL` (at least one) plus optional gatewayId.
/// From reference/packages/llm/src/providers/cloudflare.ts (`GatewayURL`)
#[derive(Debug, Clone, Default)]
pub struct GatewayUrl {
    pub account_id: Option<String>,
    pub base_url: Option<String>,
    pub gateway_id: Option<String>,
}

/// `AIGatewayOptions`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`AIGatewayOptions`)
#[derive(Debug, Clone, Default)]
pub struct AIGatewayOptions {
    pub url: GatewayUrl,
    pub api_key: Option<String>,
    pub gateway_api_key: Option<String>,
    pub auth: Option<Auth>,
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

/// `WorkersAIOptions`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`WorkersAIOptions`)
#[derive(Debug, Clone, Default)]
pub struct WorkersAIOptions {
    pub account_id: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub auth: Option<Auth>,
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

/// `aiGatewayBaseURL`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`aiGatewayBaseURL`)
pub fn ai_gateway_base_url(input: &GatewayUrl) -> Result<String, String> {
    if let Some(base_url) = &input.base_url {
        return Ok(base_url.clone());
    }
    let account_id = input.account_id.as_ref().ok_or_else(|| {
        "CloudflareAIGateway.configure requires accountId unless baseURL is supplied".to_string()
    })?;
    let gateway_id = input.gateway_id.clone().unwrap_or_default().trim().to_string();
    let gateway_id = if gateway_id.is_empty() { "default".to_string() } else { gateway_id };
    Ok(format!(
        "https://gateway.ai.cloudflare.com/v1/{}/{}/compat",
        urlencode(account_id),
        urlencode(&gateway_id)
    ))
}

fn urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn ai_gateway_auth(input: &AIGatewayOptions) -> Auth {
    if let Some(auth) = &input.auth {
        return auth.clone();
    }
    let mut gateway = optional(input.gateway_api_key.clone(), "gatewayApiKey")
        .or_else(Credential::Env("CLOUDFLARE_API_TOKEN".to_string()))
        .or_else(Credential::Env("CF_AIG_TOKEN".to_string()))
        .bearer_header_auth("cf-aig-authorization");
    if input.api_key.is_none() {
        return gateway;
    }
    if input.gateway_api_key.is_none() {
        return Credential::Value(input.api_key.clone().unwrap()).bearer_auth();
    }
    let gateway_key = input.gateway_api_key.clone().unwrap();
    gateway = Credential::Value(gateway_key).bearer_header_auth("cf-aig-authorization").and_then(
        Credential::Value(input.api_key.clone().unwrap()).bearer_auth(),
    );
    gateway
}

/// `workersAIBaseURL`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`workersAIBaseURL`)
pub fn workers_ai_base_url(input: &WorkersAIOptions) -> Result<String, String> {
    if let Some(base_url) = &input.base_url {
        return Ok(base_url.clone());
    }
    let account_id = input.account_id.as_ref().ok_or_else(|| {
        "CloudflareWorkersAI.configure requires accountId unless baseURL is supplied".to_string()
    })?;
    Ok(format!("https://api.cloudflare.com/client/v4/accounts/{}/ai/v1", urlencode(account_id)))
}

fn workers_ai_auth(input: &WorkersAIOptions) -> Auth {
    AuthOptions::bearer(input.auth.clone(), input.api_key.clone(), &WORKERS_AI_AUTH_ENV_VARS)
}

/// `aiGatewayRoute`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`aiGatewayRoute`)
pub fn ai_gateway_route() -> Route {
    let mut patch = RoutePatch::empty();
    patch.id = Some("cloudflare-ai-gateway".to_string());
    patch.provider = Some(AI_GATEWAY_ID.to_string());
    openai_compatible_chat::route().with(patch)
}

/// `workersAIRoute`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`workersAIRoute`)
pub fn workers_ai_route() -> Route {
    let mut patch = RoutePatch::empty();
    patch.id = Some("cloudflare-workers-ai".to_string());
    patch.provider = Some(WORKERS_AI_ID.to_string());
    openai_compatible_chat::route().with(patch)
}

/// `CloudflareAIGateway.configure(options)`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`configureAIGateway`)
pub fn configure_ai_gateway(options: AIGatewayOptions) -> CloudflareProvider {
    let base_url = ai_gateway_base_url(&options.url).unwrap_or_default();
    let mut patch = RoutePatch::empty();
    patch.auth = Some(ai_gateway_auth(&options));
    patch.endpoint = Some(EndpointPatch::base_url(base_url));
    patch.headers = options.headers.clone();
    patch.limits = options.limits.clone();
    patch.generation = options.generation.clone();
    patch.provider_options = options.provider_options.clone();
    patch.http = options.http.clone();
    let route = Arc::new(ai_gateway_route().with(patch));
    let model = move |model_id: String| -> Model {
        route.model(RouteModelInput { id: model_id, provider: Some(AI_GATEWAY_ID.to_string()), defaults: None, compatibility: None }).unwrap()
    };
    CloudflareProvider { id: AI_GATEWAY_ID.to_string(), model: Arc::new(model) }
}

/// `CloudflareWorkersAI.configure(options)`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`configureWorkersAI`)
pub fn configure_workers_ai(options: WorkersAIOptions) -> CloudflareProvider {
    let base_url = workers_ai_base_url(&options).unwrap_or_default();
    let mut patch = RoutePatch::empty();
    patch.auth = Some(workers_ai_auth(&options));
    patch.endpoint = Some(EndpointPatch::base_url(base_url));
    patch.headers = options.headers.clone();
    patch.limits = options.limits.clone();
    patch.generation = options.generation.clone();
    patch.provider_options = options.provider_options.clone();
    patch.http = options.http.clone();
    let route = Arc::new(workers_ai_route().with(patch));
    let model = move |model_id: String| -> Model {
        route.model(RouteModelInput { id: model_id, provider: Some(WORKERS_AI_ID.to_string()), defaults: None, compatibility: None }).unwrap()
    };
    CloudflareProvider { id: WORKERS_AI_ID.to_string(), model: Arc::new(model) }
}

/// Provider handle.
#[derive(Clone)]
pub struct CloudflareProvider {
    pub id: String,
    pub model: Arc<dyn Fn(String) -> Model + Send + Sync>,
}

impl CloudflareProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.model)(id.into())
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/cloudflare.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![ai_gateway_route(), workers_ai_route()]
}

fn optional(value: Option<String>, source: &str) -> Credential {
    crate::route::auth::optional(value, source)
}

trait CredentialExt {
    fn bearer_header_auth(self, name: &str) -> Auth;
    fn bearer_auth(self) -> Auth;
}

impl CredentialExt for Credential {
    fn bearer_header_auth(self, name: &str) -> Auth {
        crate::route::auth::bearer_header(name, self)
    }

    fn bearer_auth(self) -> Auth {
        crate::route::auth::bearer(self)
    }
}
