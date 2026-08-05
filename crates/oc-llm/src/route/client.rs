//! Route composition and the `LLMClient` prepare/stream/generate surface.
//! From reference/packages/llm/src/route/client.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::stream::{BoxStream, StreamExt, TryStreamExt};
use serde_json::Value;

use super::auth::Auth;
use super::endpoint::{self, Endpoint, EndpointPatch};
use super::executor::Executor;
use super::framing::Framing;
use super::protocol::{FramePayload, ProtocolStream};
use super::transport::{self, HttpPrepared, HttpRequestValue};
use crate::cache_policy::apply_cache_policy;
use crate::schema::{
    merge_generation_options, merge_http_options, merge_provider_options, GenerationOptions, HttpOptions, LlmError,
    LlmEvent, LlmRequest, LlmResponse, Message, Model, ModelCompatibility, ModelDefaults, ModelLimits, ModelInput,
    ModelSerializable, PreparedRequest, ProviderOptions, SystemPart, ToolDefinition, response_complete, response_empty,
    response_reduce,
};

/// `RouteBody` — body construction for a route.
/// From reference/packages/llm/src/route/client.ts (`RouteBody`)
pub type BodyFn = Arc<dyn Fn(&LlmRequest) -> Result<Value, LlmError> + Send + Sync>;

/// `Protocol` — semantic API contract for one model server family.
/// From reference/packages/llm/src/route/protocol.ts
#[derive(Clone)]
pub struct Protocol {
    pub id: String,
    pub body: BodyFn,
    pub stream: Arc<dyn ProtocolStream>,
}

impl Protocol {
    pub fn make(
        id: impl Into<String>,
        body: BodyFn,
        stream: Arc<dyn ProtocolStream>,
    ) -> Protocol {
        Protocol { id: id.into(), body, stream }
    }
}

/// `RouteDefaults`.
/// From reference/packages/llm/src/route/client.ts (`RouteDefaults`)
#[derive(Debug, Clone, Default)]
pub struct RouteDefaults {
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

/// `RouteDefaultsInput`.
/// From reference/packages/llm/src/route/client.ts (`RouteDefaultsInput`)
#[derive(Debug, Clone, Default)]
pub struct RouteDefaultsInput {
    pub headers: Option<BTreeMap<String, String>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

fn merge_headers(items: &[Option<&BTreeMap<String, String>>]) -> Option<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for item in items.iter().flatten() {
        for (k, v) in *item {
            result.insert(k.clone(), v.clone());
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn merge_route_defaults(base: Option<&RouteDefaults>, patch: &RouteDefaultsInput) -> RouteDefaults {
    let headers = merge_headers(&[base.and_then(|b| b.headers.as_ref()), patch.headers.as_ref()]);
    RouteDefaults {
        headers: headers.clone(),
        limits: patch.limits.clone().or_else(|| base.and_then(|b| b.limits.clone())),
        generation: merge_generation_options(&[
            base.and_then(|b| b.generation.as_ref()),
            patch.generation.as_ref(),
        ]),
        provider_options: merge_provider_options(&[
            base.and_then(|b| b.provider_options.as_ref()),
            patch.provider_options.as_ref(),
        ]),
        http: merge_http_options(&[
            base.and_then(|b| b.http.clone()),
            patch.http.clone(),
            headers.map(|headers| HttpOptions { body: None, headers: Some(headers), query: None }),
        ]),
    }
}

/// `Route` — the runnable composition of protocol, endpoint, auth, framing.
/// From reference/packages/llm/src/route/client.ts (`Route`)
#[derive(Clone)]
pub struct Route {
    pub id: String,
    pub provider: Option<String>,
    pub protocol: Protocol,
    pub endpoint: Endpoint,
    pub auth: Auth,
    pub framing: Framing,
    pub defaults: RouteDefaults,
    pub headers: Option<Arc<dyn Fn(&LlmRequest) -> BTreeMap<String, String> + Send + Sync>>,
}

impl std::fmt::Debug for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Route")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("protocol", &self.protocol.id)
            .finish()
    }
}

/// `RouteModelInput`.
/// From reference/packages/llm/src/route/client.ts (`RouteModelInput`)
#[derive(Debug, Clone, Default)]
pub struct RouteModelInput {
    pub id: String,
    pub provider: Option<String>,
    pub defaults: Option<ModelDefaults>,
    pub compatibility: Option<ModelCompatibility>,
}

impl Route {
    /// `Route.make(...)`.
    /// From reference/packages/llm/src/route/client.ts (`make`)
    pub fn make(input: RouteMakeInput) -> Route {
        Route {
            id: input.id,
            provider: input.provider,
            protocol: input.protocol,
            endpoint: input.endpoint,
            auth: input.auth.unwrap_or_else(Auth::none),
            framing: input.framing.unwrap_or(Framing::Sse),
            defaults: merge_route_defaults(None, &input.defaults.unwrap_or_default()),
            headers: input.headers,
        }
    }

    /// `Route.make` with an explicit transport (reserved for WebSocket).
    pub fn make_with_transport(input: RouteMakeInput, _transport: Transport) -> Route {
        Route::make(input)
    }

    /// `route.with(patch)`.
    /// From reference/packages/llm/src/route/client.ts (`Route.with`)
    pub fn with(&self, patch: RoutePatch) -> Route {
        let mut defaults_patch = RouteDefaultsInput::default();
        defaults_patch.headers = patch.headers;
        defaults_patch.limits = patch.limits;
        defaults_patch.generation = patch.generation;
        defaults_patch.provider_options = patch.provider_options;
        defaults_patch.http = patch.http;

        let endpoint = match &patch.endpoint {
            Some(endpoint_patch) => endpoint::merge(&self.endpoint, endpoint_patch.clone()),
            None => self.endpoint.clone(),
        };

        Route {
            id: patch.id.unwrap_or_else(|| self.id.clone()),
            provider: patch.provider.or_else(|| self.provider.clone()),
            protocol: self.protocol.clone(),
            endpoint,
            auth: patch.auth.unwrap_or_else(|| self.auth.clone()),
            framing: self.framing,
            defaults: merge_route_defaults(Some(&self.defaults), &defaults_patch),
            headers: patch.headers_fn.or_else(|| self.headers.clone()),
        }
    }

    /// `route.model(input)`.
    /// From reference/packages/llm/src/route/client.ts (`makeRouteModel`)
    pub fn model(&self, input: RouteModelInput) -> Result<Model, String> {
        let provider = self.provider.clone().or(input.provider.clone());
        let Some(provider) = provider else {
            return Err(format!("Route.model({}) requires a provider", self.id));
        };
        if self.endpoint.base_url.is_none() {
            return Err(format!(
                "Route.model({}) requires an endpoint baseURL — configure it on the route first",
                self.id
            ));
        }
        Ok(Model::make(ModelInput {
            id: input.id,
            provider,
            route: Arc::new(self.clone()),
            defaults: input.defaults,
            compatibility: input.compatibility,
        }))
    }
}

/// `Transport` — reserved for non-HTTP transports (WebSocket, Bedrock binary).
/// From reference/packages/llm/src/route/transport/index.ts
#[derive(Debug, Clone, Copy)]
pub enum Transport {
    /// POST + framing over HTTP.
    HttpJson,
}

/// `RouteMakeInput`.
/// From reference/packages/llm/src/route/client.ts (`MakeInput`)
pub struct RouteMakeInput {
    pub id: String,
    pub provider: Option<String>,
    pub protocol: Protocol,
    pub endpoint: Endpoint,
    pub auth: Option<Auth>,
    pub framing: Option<Framing>,
    pub headers: Option<Arc<dyn Fn(&LlmRequest) -> BTreeMap<String, String> + Send + Sync>>,
    pub defaults: Option<RouteDefaultsInput>,
}

impl RouteMakeInput {
    pub fn new(id: impl Into<String>, protocol: Protocol, endpoint: Endpoint) -> RouteMakeInput {
        RouteMakeInput {
            id: id.into(),
            provider: None,
            protocol,
            endpoint,
            auth: None,
            framing: None,
            headers: None,
            defaults: None,
        }
    }
}

/// `RoutePatch`.
/// From reference/packages/llm/src/route/client.ts (`RoutePatch`)
#[derive(Clone, Default)]
pub struct RoutePatch {
    pub id: Option<String>,
    pub provider: Option<String>,
    pub auth: Option<Auth>,
    pub endpoint: Option<EndpointPatch>,
    pub headers: Option<BTreeMap<String, String>>,
    pub headers_fn: Option<Arc<dyn Fn(&LlmRequest) -> BTreeMap<String, String> + Send + Sync>>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

impl RoutePatch {
    pub fn empty() -> RoutePatch {
        RoutePatch::default()
    }
}

/// Compiled request before transport execution.
pub struct Compiled {
    pub request: LlmRequest,
    pub route: Arc<Route>,
    pub body: Value,
    pub prepared: HttpPrepared,
}

/// Resolve route/model/request option precedence.
/// From reference/packages/llm/src/route/client.ts (`resolveRequestOptions`)
fn resolve_request_options(request: &LlmRequest) -> LlmRequest {
    let route_defaults = &request.model.route.defaults;
    let model_defaults = request.model.defaults.clone();
    let generation = merge_generation_options(&[
        route_defaults.generation.as_ref(),
        model_defaults.as_ref().and_then(|d| d.generation.as_ref()),
        request.generation.as_ref(),
    ]);
    let generation = Some(generation.unwrap_or_default());
    let provider_options = merge_provider_options(&[
        route_defaults.provider_options.as_ref(),
        model_defaults.as_ref().and_then(|d| d.provider_options.as_ref()),
        request.provider_options.as_ref(),
    ]);
    let http = merge_http_options(&[
        route_defaults.http.clone(),
        model_defaults.as_ref().and_then(|d| d.http.clone()),
        request.http.clone(),
    ]);

    let mut patch = crate::schema::LlmRequestPatch::empty();
    patch.generation = Some(generation);
    patch.provider_options = Some(provider_options);
    patch.http = Some(http);
    LlmRequest::update(request, patch)
}

/// `compile` — resolve options, apply cache policy, build the provider body,
/// and prepare the transport.
/// From reference/packages/llm/src/route/client.ts (`compile`)
pub fn compile(request: &LlmRequest) -> Result<Compiled, LlmError> {
    let resolved = apply_cache_policy(&resolve_request_options(request));
    let route = resolved.model.route.clone();
    let body = (route.protocol.body)(&resolved)?;
    let prepared = transport_prepare(&route, &body, &resolved)?;
    Ok(Compiled { request: resolved, route, body, prepared })
}

fn transport_prepare(route: &Route, body: &Value, request: &LlmRequest) -> Result<HttpPrepared, LlmError> {
    let parts = transport::json_request_parts(body, request, &route.endpoint, &route.auth, route.headers.as_ref())?;
    Ok(HttpPrepared {
        request: HttpRequestValue { url: parts.url, body: parts.body_text, headers: parts.headers },
        framing: route.framing,
    })
}

/// `LlmClient` — the public execution surface.
/// From reference/packages/llm/src/route/client.ts (`LLMClient`)
#[derive(Clone)]
pub struct LlmClient {
    pub executor: Executor,
}

impl Default for LlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient {
    pub fn new() -> LlmClient {
        LlmClient { executor: Executor::new(reqwest::Client::new()) }
    }

    pub fn with_http_client(client: reqwest::Client) -> LlmClient {
        LlmClient { executor: Executor::new(client) }
    }

    /// `LLMClient.prepare(request)` — compile without sending.
    /// From reference/packages/llm/src/route/client.ts (`prepare`)
    pub fn prepare(&self, request: &LlmRequest) -> Result<PreparedRequest, LlmError> {
        let compiled = compile(request)?;
        Ok(PreparedRequest {
            id: compiled.request.id.clone().unwrap_or_else(|| "request".to_string()),
            route: compiled.route.id.clone(),
            protocol: compiled.route.protocol.id.clone(),
            model: ModelSerializable::from_model(&compiled.request.model),
            body: compiled.body,
            metadata: Some(
                serde_json::Map::from_iter([("transport".to_string(), Value::String("http-json".to_string()))]),
            ),
        })
    }

    /// `LLMClient.stream(request)`.
    /// From reference/packages/llm/src/route/client.ts (`stream`)
    pub fn stream(&self, request: LlmRequest) -> BoxStream<'static, Result<LlmEvent, LlmError>> {
        let executor = self.executor.clone();
        let compiled = compile(&request);
        match compiled {
            Err(error) => Box::pin(futures::stream::once(async move { Err(error) })),
            Ok(compiled) => {
                let route_key = format!("{}/{}", compiled.request.model.provider, compiled.request.model.route.id);
                let stream = futures::stream::once(async move {
                    let response = executor.execute(&compiled.prepared.request).await?;
                    let body = response
                        .bytes_stream()
                        .map_err(move |_error| {
                            LlmError::event_error(&route_key, format!("Failed to read {} stream", route_key), None)
                        })
                        .boxed();
                    let frames = transport::frames(&compiled.prepared, &compiled.request, body);
                    let proto = super::protocol::ProtoStream::new(
                        frames,
                        compiled.route.protocol.stream.clone(),
                        compiled.request.clone(),
                    );
                    Ok(proto) as Result<super::protocol::ProtoStream, LlmError>
                });
                Box::pin(stream.try_flatten())
            }
        }
    }

    /// `LLMClient.generate(request)`.
    /// From reference/packages/llm/src/route/client.ts (`generate`)
    pub async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let route = format!("{}/{}", request.model.provider, request.model.route.id);
        let mut state = response_empty();
        let mut stream = self.stream(request);
        while let Some(event) = stream.next().await {
            let event = event?;
            state = response_reduce(&state, &event);
        }
        if let Some(response) = response_complete(&state) {
            return Ok(response);
        }
        Err(crate::shared::event_error(&route, "Provider stream ended without a terminal finish event", None))
    }
}

#[allow(unused)]
fn _message_marker(m: &Message) {
    let _ = m;
}

#[allow(unused)]
fn _system_marker(s: &SystemPart) {
    let _ = s;
}

#[allow(unused)]
fn _tool_marker(t: &ToolDefinition) {
    let _ = t;
}

#[allow(unused)]
fn _frame_marker(f: &FramePayload) {
    let _ = f;
}

/// `LLMClient` namespace marker matching `LLMClient.prepare/stream/generate`.
pub mod _client {
    pub use super::{LlmClient, Route};
}
