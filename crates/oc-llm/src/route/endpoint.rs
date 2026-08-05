//! Declarative URL construction for one route.
//! From reference/packages/llm/src/route/endpoint.ts

use url::Url;

use crate::shared::trim_base_url;
use crate::schema::LlmRequest;

/// `EndpointInput`.
/// From reference/packages/llm/src/route/endpoint.ts
pub struct EndpointInput<'a> {
    pub request: &'a LlmRequest,
    pub body: &'a serde_json::Value,
}

/// `Endpoint` — base URL, path, and query for a route.
/// From reference/packages/llm/src/route/endpoint.ts
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: Option<String>,
    pub path: EndpointPath,
    pub query: Option<std::collections::BTreeMap<String, String>>,
}

/// `EndpointPart` — a path string or a function of the request/body.
#[derive(Clone)]
pub enum EndpointPath {
    Static(String),
    Dynamic(Arc<dyn Fn(&EndpointInput) -> String + Send + Sync>),
}

impl std::fmt::Debug for EndpointPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointPath::Static(path) => write!(f, "EndpointPath::Static({path})"),
            EndpointPath::Dynamic(_) => f.write_str("EndpointPath::Dynamic"),
        }
    }
}

use std::sync::Arc;

/// `Endpoint.path(value, options)`.
/// From reference/packages/llm/src/route/endpoint.ts (`path`)
pub fn path(value: impl Into<String>, options: EndpointOptions) -> Endpoint {
    Endpoint {
        base_url: options.base_url,
        path: EndpointPath::Static(value.into()),
        query: options.query,
    }
}

/// `Endpoint.path_dynamic(fn)` — for paths embedding the model id or body.
pub fn path_dynamic(
    f: impl Fn(&EndpointInput) -> String + Send + Sync + 'static,
    options: EndpointOptions,
) -> Endpoint {
    Endpoint { base_url: options.base_url, path: EndpointPath::Dynamic(Arc::new(f)), query: options.query }
}

pub struct EndpointOptions {
    pub base_url: Option<String>,
    pub query: Option<std::collections::BTreeMap<String, String>>,
}

impl EndpointOptions {
    pub fn none() -> Self {
        EndpointOptions { base_url: None, query: None }
    }
}

/// `Endpoint.merge(base, patch)`.
/// From reference/packages/llm/src/route/endpoint.ts (`merge`)
pub fn merge(base: &Endpoint, patch: EndpointPatch) -> Endpoint {
    let mut query = base.query.clone();
    if let Some(patch_query) = &patch.query {
        let mut merged = query.unwrap_or_default();
        for (k, v) in patch_query {
            merged.insert(k.clone(), v.clone());
        }
        query = Some(merged);
    }
    Endpoint {
        base_url: patch.base_url.or_else(|| base.base_url.clone()),
        path: patch.path.unwrap_or_else(|| base.path.clone()),
        query,
    }
}

/// `EndpointPatch` — partial endpoint.
#[derive(Debug, Clone, Default)]
pub struct EndpointPatch {
    pub base_url: Option<String>,
    pub path: Option<EndpointPath>,
    pub query: Option<std::collections::BTreeMap<String, String>>,
}

impl EndpointPatch {
    pub fn base_url(url: impl Into<String>) -> EndpointPatch {
        EndpointPatch { base_url: Some(url.into()), ..Default::default() }
    }

    pub fn query(query: std::collections::BTreeMap<String, String>) -> EndpointPatch {
        EndpointPatch { query: Some(query), ..Default::default() }
    }
}

/// `Endpoint.render(endpoint, input)`.
/// From reference/packages/llm/src/route/endpoint.ts (`render`)
pub fn render(endpoint: &Endpoint, input: &EndpointInput) -> Result<Url, crate::schema::LlmError> {
    let part = match &endpoint.path {
        EndpointPath::Static(path) => path.clone(),
        EndpointPath::Dynamic(f) => f(input),
    };
    let url = format!("{}{}", trim_base_url(endpoint.base_url.as_deref().unwrap_or("")), part);
    let mut parsed = Url::parse(&url).map_err(|error| {
        crate::shared::invalid_request(format!("Invalid endpoint URL {}: {}", url, error))
    })?;
    if let Some(query) = &endpoint.query {
        for (key, value) in query {
            parsed.query_pairs_mut().append_pair(key, value);
        }
    }
    Ok(parsed)
}
