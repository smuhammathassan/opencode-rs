//! Low-level HTTP transport shared by all client groups.
//! Mirrors the `prepare`, `execute`, `json`, and `responseError` helpers in
//! `reference/packages/client/src/generated/client.ts`.

use crate::error::{decode_api_error, ClientError, Error};
use crate::sse::SseDecoder;
use reqwest::header::{HeaderMap, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;
use url::Url;

/// Client construction options. Mirrors `ClientOptions` in
/// `reference/packages/client/src/generated/client.ts` (`fetch` maps to a custom
/// `reqwest::Client`).
#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub base_url: Url,
    pub headers: HeaderMap,
    pub http: Option<reqwest::Client>,
}

impl Default for ClientOptions {
    fn default() -> Self {
        ClientOptions {
            base_url: Url::parse("http://localhost:3000").expect("valid default url"),
            headers: HeaderMap::new(),
            http: None,
        }
    }
}

/// Per-request options. Mirrors `RequestOptions` in
/// `reference/packages/client/src/generated/client.ts`.
#[derive(Debug, Clone, Default)]
pub struct RequestOptions {
    pub headers: HeaderMap,
    pub timeout: Option<Duration>,
    pub retry: Option<RetryPolicy>,
}

/// Optional retry policy. The reference Protocol exposes no client retry
/// middleware; this is a client-side extension for transport and 5xx failures.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
}

/// A prepared request: HTTP verb, path, query, and JSON body.
#[derive(Debug, Clone)]
pub(crate) struct RequestDescriptor {
    pub method: reqwest::Method,
    pub path: String,
    pub query: Vec<(String, Value)>,
    pub body: Option<Value>,
    pub success_status: u16,
    pub declared_statuses: &'static [u16],
    pub empty: bool,
}

/// Cloneable HTTP transport.
#[derive(Clone)]
pub(crate) struct Transport {
    pub http: reqwest::Client,
    pub base_url: Url,
    pub headers: HeaderMap,
}

impl Transport {
    pub(crate) fn new(options: &ClientOptions) -> Result<Self, reqwest::Error> {
        let http = match &options.http {
            Some(client) => client.clone(),
            None => reqwest::Client::builder().build()?,
        };
        Ok(Transport {
            http,
            base_url: options.base_url.clone(),
            headers: options.headers.clone(),
        })
    }

    pub(crate) async fn execute<T: DeserializeOwned>(
        &self,
        desc: &RequestDescriptor,
        options: Option<&RequestOptions>,
    ) -> Result<T, Error> {
        let attempt = || {
            let desc = desc.clone();
            let options = options.cloned();
            let this = self.clone();
            let future: futures::future::BoxFuture<'static, Result<T, Error>> =
                Box::pin(async move { this.execute_inner(&desc, options.as_ref()).await });
            future
        };
        self.with_retry(options, attempt).await
    }

    pub(crate) async fn start_sse(
        &self,
        desc: &RequestDescriptor,
        options: Option<&RequestOptions>,
    ) -> Result<SseDecoder, Error> {
        let attempt = || {
            let desc = desc.clone();
            let options = options.cloned();
            let this = self.clone();
            let future: futures::future::BoxFuture<'static, Result<SseDecoder, Error>> =
                Box::pin(async move { this.start_sse_inner(&desc, options.as_ref()).await });
            future
        };
        self.with_retry(options, attempt).await
    }

    async fn with_retry<T>(
        &self,
        options: Option<&RequestOptions>,
        attempt: impl Fn() -> futures::future::BoxFuture<'static, Result<T, Error>>,
    ) -> Result<T, Error> {
        let Some(policy) = options.and_then(|options| options.retry) else {
            return attempt().await;
        };
        let mut errors = std::vec::Vec::new();
        for attempt_index in 0..policy.max_attempts {
            match attempt().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    let retryable = is_retryable(&err);
                    if !retryable || attempt_index + 1 >= policy.max_attempts {
                        return Err(err);
                    }
                    let delay = policy
                        .base_delay
                        .saturating_mul(2u32.saturating_pow(attempt_index));
                    tokio::time::sleep(delay).await;
                    errors.push(err);
                }
            }
        }
        Err(errors
            .pop()
            .unwrap_or_else(|| Error::Client(ClientError::UnexpectedStatus(0))))
    }

    async fn execute_inner<T: DeserializeOwned>(
        &self,
        desc: &RequestDescriptor,
        options: Option<&RequestOptions>,
    ) -> Result<T, Error> {
        let request = self.build_request(desc, options)?;
        let response = self
            .http
            .execute(request)
            .await
            .map_err(|err| ClientError::Transport(err))?;
        let status = response.status().as_u16();
        if status != desc.success_status {
            return Err(self.response_error(desc, response).await);
        }
        if desc.empty {
            return Ok(serde_json::from_value(Value::Null).map_err(ClientError::from)?);
        }
        let text = self.read_json_body(response).await?;
        if text.is_empty() {
            return Err(ClientError::MalformedResponse(None).into());
        }
        let value: Value =
            serde_json::from_str(&text).map_err(|err| ClientError::MalformedResponse(Some(err)))?;
        serde_json::from_value(value)
            .map_err(|err| ClientError::MalformedResponse(Some(err)).into())
    }

    async fn start_sse_inner(
        &self,
        desc: &RequestDescriptor,
        options: Option<&RequestOptions>,
    ) -> Result<SseDecoder, Error> {
        let request = self.build_request(desc, options)?;
        let response = self
            .http
            .execute(request)
            .await
            .map_err(|err| ClientError::Transport(err))?;
        let status = response.status().as_u16();
        if status != desc.success_status {
            return Err(self.response_error(desc, response).await);
        }
        if !is_event_stream(&response) {
            return Err(ClientError::UnsupportedContentType.into());
        }
        Ok(SseDecoder::new(response))
    }

    fn build_request(
        &self,
        desc: &RequestDescriptor,
        options: Option<&RequestOptions>,
    ) -> Result<reqwest::Request, ClientError> {
        let url = self.build_url(desc);
        let mut builder = self.http.request(desc.method.clone(), url);
        builder = builder.headers(self.headers.clone());
        if let Some(options) = options {
            if !options.headers.is_empty() {
                builder = builder.headers(options.headers.clone());
            }
            if let Some(timeout) = options.timeout {
                builder = builder.timeout(timeout);
            }
        }
        if let Some(body) = &desc.body {
            builder = builder.json(body);
        }
        builder.build().map_err(|err| ClientError::Transport(err))
    }

    fn build_url(&self, desc: &RequestDescriptor) -> Url {
        let mut url = self
            .base_url
            .join(&desc.path)
            .unwrap_or_else(|_| Url::parse("http://invalid").expect("static fallback"));
        if !desc.query.is_empty() {
            let mut pairs = std::vec::Vec::new();
            for (key, value) in &desc.query {
                append_query(&mut pairs, key, value);
            }
            url.query_pairs_mut()
                .extend_pairs(pairs.into_iter().map(|(key, value)| (key, value)));
        }
        url
    }

    async fn response_error(&self, desc: &RequestDescriptor, response: reqwest::Response) -> Error {
        let status = response.status().as_u16();
        if desc.declared_statuses.contains(&status) {
            match self.read_json_body(response).await {
                Ok(text) => match serde_json::from_str::<Value>(&text) {
                    Ok(value) => Error::Api(decode_api_error(value)),
                    Err(err) => Error::Client(ClientError::MalformedResponse(Some(err))),
                },
                Err(err) => err,
            }
        } else {
            Error::Client(ClientError::UnexpectedStatus(status))
        }
    }

    async fn read_json_body(&self, response: reqwest::Response) -> Result<String, Error> {
        if !is_json(&response) {
            return Err(ClientError::UnsupportedContentType.into());
        }
        response
            .text()
            .await
            .map_err(|err| ClientError::Transport(err).into())
    }
}

/// Mirror of `appendQuery` in `reference/packages/client/src/generated/client.ts`:
/// null is skipped, arrays repeat the key, objects recurse as `key[child]`.
fn append_query(out: &mut Vec<(String, String)>, key: &str, value: &Value) {
    match value {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                append_query(out, key, item);
            }
        }
        Value::Object(map) => {
            for (child, item) in map {
                append_query(out, &format!("{key}[{child}]"), item);
            }
        }
        Value::Bool(value) => out.push((key.to_string(), value.to_string())),
        Value::Number(value) => out.push((key.to_string(), value.to_string())),
        Value::String(value) => out.push((key.to_string(), value.clone())),
    }
}

fn is_json(response: &reqwest::Response) -> bool {
    match response.headers().get(CONTENT_TYPE) {
        Some(value) => {
            let raw = value.to_str().unwrap_or_default();
            media_type(raw) == "application/json" || raw.contains("+json")
        }
        None => false,
    }
}

fn is_event_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| media_type(value) == "text/event-stream")
        .unwrap_or(false)
}

fn media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}

fn is_retryable(err: &Error) -> bool {
    match err {
        Error::Client(ClientError::Transport(_)) => true,
        Error::Client(ClientError::UnexpectedStatus(status)) => *status >= 500,
        _ => false,
    }
}
