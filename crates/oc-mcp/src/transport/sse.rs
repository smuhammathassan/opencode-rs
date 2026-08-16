//! SSE (legacy HTTP) client transport.
//!
//! From `@modelcontextprotocol/sdk@1.29.0` `client/sse.js`, used as a fallback
//! in `reference/packages/opencode/src/mcp/index.ts` (`connectRemote`) when the
//! Streamable HTTP transport cannot connect. The server streams JSON-RPC
//! messages as `message` events and may advertise a POST endpoint via an
//! `endpoint` event.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client as HttpClient;
use tokio::sync::{mpsc, Mutex};
use tracing::debug;
use url::Url;

use super::{MessageReceiver, OpenFlag, SseParser, Transport};
use crate::jsonrpc::Message;
use crate::oauth::AuthOptions;
use crate::oauth_provider::OAuthClientProvider;
use crate::util::BoxFuture;
use crate::Result;

pub struct SSEClientTransport {
    url: Url,
    headers: HashMap<String, String>,
    auth_provider: Option<Arc<dyn OAuthClientProvider>>,
    auth_client: Arc<crate::oauth::AuthClient>,
    http: HttpClient,
    endpoint: Arc<Mutex<Option<Url>>>,
    last_event_id: Arc<Mutex<Option<String>>>,
    has_completed_auth_flow: Arc<AtomicBool>,
    tx: Mutex<Option<mpsc::UnboundedSender<Message>>>,
    open: OpenFlag,
}

impl SSEClientTransport {
    pub fn new(
        url: Url,
        headers: Option<HashMap<String, String>>,
        auth_provider: Option<Arc<dyn OAuthClientProvider>>,
    ) -> Self {
        SSEClientTransport {
            url,
            headers: headers.unwrap_or_default(),
            auth_provider,
            auth_client: Arc::new(crate::oauth::AuthClient::new()),
            http: crate::oauth::http_client(),
            endpoint: Arc::new(Mutex::new(None)),
            last_event_id: Arc::new(Mutex::new(None)),
            has_completed_auth_flow: Arc::new(AtomicBool::new(false)),
            tx: Mutex::new(None),
            open: OpenFlag::new(),
        }
    }

    async fn common_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        if let Some(provider) = &self.auth_provider {
            if let Some(tokens) = provider.tokens().await? {
                if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", tokens.access_token))
                {
                    headers.insert(AUTHORIZATION, value);
                }
            }
        }
        for (key, value) in &self.headers {
            if let (Ok(key), Ok(value)) = (
                HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(key, value);
            }
        }
        Ok(headers)
    }

    async fn handle_auth(&self) -> Result<()> {
        let Some(provider) = &self.auth_provider else {
            return Ok(());
        };
        if self.has_completed_auth_flow.load(Ordering::SeqCst) {
            return Err(crate::Error::unauthorized(
                "server returned 401 after successful auth",
            ));
        }
        let options = AuthOptions {
            server_url: self.url.clone(),
            authorization_code: None,
            scope: None,
            resource_metadata_url: None,
        };
        match self.auth_client.auth(provider.as_ref(), &options).await {
            Ok(_outcome) => {
                self.has_completed_auth_flow.store(true, Ordering::SeqCst);
                Ok(())
            }
            Err(crate::Error::Unauthorized { .. }) => {
                Err(crate::Error::unauthorized("authorization required"))
            }
            Err(error) => Err(error),
        }
    }
}

impl Transport for SSEClientTransport {
    fn start(&self) -> BoxFuture<'_, Result<MessageReceiver>> {
        Box::pin(async move {
            let (tx, rx) = mpsc::unbounded_channel();
            *self.tx.lock().await = Some(tx.clone());
            let response = self.open_event_stream().await?;

            let url = self.url.clone();
            let headers = self.headers.clone();
            let http = self.http.clone();
            let auth_provider = self.auth_provider.clone();
            let auth_client = self.auth_client.clone();
            let endpoint = self.endpoint.clone();
            let last_event_id = self.last_event_id.clone();
            let open = self.open.clone();

            tokio::spawn(async move {
                let mut response: Option<reqwest::Response> = Some(response);
                loop {
                    if open.is_closed() {
                        break;
                    }
                    let Some(current) = response.take() else {
                        match open_event_stream_again(
                            &url,
                            &headers,
                            &http,
                            &auth_provider,
                            &auth_client,
                            &last_event_id,
                        )
                        .await
                        {
                            Ok(new_response) => response = Some(new_response),
                            Err(error) => {
                                if matches!(error, crate::Error::Unauthorized { .. }) {
                                    debug!("MCP SSE stream requires authorization, giving up");
                                    break;
                                }
                                debug!("MCP SSE reconnect failed: {error}");
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }
                        continue;
                    };
                    let done = consume_events(&url, current, &tx, &endpoint, &last_event_id).await;
                    if done || open.is_closed() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                drop(tx);
            });

            Ok(rx)
        })
    }

    fn send(&self, message: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if self.open.is_closed() {
                return Err(crate::Error::message("transport closed"));
            }
            let headers = self.common_headers().await?;
            let endpoint = self
                .endpoint
                .lock()
                .await
                .clone()
                .unwrap_or_else(|| self.url.clone());
            let body = serde_json::to_string(&message)?;
            let response = self
                .http
                .post(endpoint)
                .headers(headers)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json, text/event-stream")
                .body(body)
                .send()
                .await?;
            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    self.handle_auth().await?;
                    return self.send(message).await;
                }
                return Err(crate::Error::message(format!(
                    "MCP server returned {status}: {text}"
                )));
            }
            if let Some(tx) = self.tx.lock().await.as_ref() {
                deliver_json_or_sse(response, tx, &self.last_event_id).await;
            }
            Ok(())
        })
    }

    fn close(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.open.mark_closed();
            self.tx.lock().await.take();
            Ok(())
        })
    }

    fn finish_auth(&self, code: &str) -> BoxFuture<'_, Result<()>> {
        let url = self.url.clone();
        let auth_client = self.auth_client.clone();
        let provider = self.auth_provider.clone();
        let code = code.to_string();
        let has_completed = self.has_completed_auth_flow.clone();
        Box::pin(async move {
            let Some(provider) = provider else {
                return Err(crate::Error::message("no OAuth provider for this server"));
            };
            auth_client
                .finish_with_code(provider.as_ref(), &url, &code)
                .await?;
            has_completed.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}

impl SSEClientTransport {
    async fn open_event_stream(&self) -> Result<reqwest::Response> {
        let headers = self.common_headers().await?;
        let response = self
            .http
            .get(self.url.clone())
            .headers(headers)
            .header(ACCEPT, "text/event-stream")
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                self.handle_auth().await?;
                let headers = self.common_headers().await?;
                let retry = self
                    .http
                    .get(self.url.clone())
                    .headers(headers)
                    .header(ACCEPT, "text/event-stream")
                    .send()
                    .await?;
                if retry.status().is_success() {
                    return Ok(retry);
                }
                let retry_status = retry.status();
                let text = retry.text().await.unwrap_or_default();
                return Err(crate::Error::message(format!(
                    "Error opening MCP SSE stream: {retry_status} {text}"
                )));
            }
            let text = response.text().await.unwrap_or_default();
            return Err(crate::Error::message(format!(
                "Error opening MCP SSE stream: {status} {text}"
            )));
        }
        Ok(response)
    }
}

async fn consume_events(
    base_url: &Url,
    response: reqwest::Response,
    tx: &mpsc::UnboundedSender<Message>,
    endpoint: &Arc<Mutex<Option<Url>>>,
    last_event_id: &Arc<Mutex<Option<String>>>,
) -> bool {
    let mut stream = response.bytes_stream();
    let mut parser = SseParser::new();
    loop {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    if let Some(id) = event.id.filter(|id| !id.is_empty()) {
                        *last_event_id.lock().await = Some(id);
                    }
                    match event.event.as_deref() {
                        Some("endpoint") => {
                            if let Ok(mut endpoint) = endpoint.try_lock() {
                                *endpoint = base_url.join(&event.data).ok();
                            }
                        }
                        _ => {
                            if let Ok(message) = serde_json::from_str::<Message>(&event.data) {
                                if tx.send(message).is_err() {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            Ok(Some(Err(error))) => {
                debug!("MCP SSE read error: {error}");
                return false;
            }
            Ok(None) => return false,
            Err(_) => {}
        }
    }
}

async fn open_event_stream_again(
    url: &Url,
    headers: &HashMap<String, String>,
    http: &HttpClient,
    auth_provider: &Option<Arc<dyn OAuthClientProvider>>,
    _auth_client: &Arc<crate::oauth::AuthClient>,
    last_event_id: &Arc<Mutex<Option<String>>>,
) -> Result<reqwest::Response> {
    let mut header_map = HeaderMap::new();
    if let Some(provider) = auth_provider {
        if let Some(tokens) = provider.tokens().await? {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)) {
                header_map.insert(AUTHORIZATION, value);
            }
        }
    }
    for (key, value) in headers {
        if let (Ok(key), Ok(value)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            header_map.insert(key, value);
        }
    }
    if let Some(last_event_id) = last_event_id.lock().await.as_deref() {
        if let Ok(value) = HeaderValue::from_str(last_event_id) {
            header_map.insert("last-event-id", value);
        }
    }
    let response = http
        .get(url.clone())
        .headers(header_map)
        .header(ACCEPT, "text/event-stream")
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(crate::Error::unauthorized("authorization required"));
        }
        return Err(crate::Error::message(format!(
            "MCP SSE reconnect failed: {status}"
        )));
    }
    Ok(response)
}

async fn deliver_json_or_sse(
    response: reqwest::Response,
    tx: &mpsc::UnboundedSender<Message>,
    last_event_id: &Arc<Mutex<Option<String>>>,
) {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if content_type.contains("text/event-stream") {
        let mut stream = response.bytes_stream();
        let mut parser = SseParser::new();
        while let Some(Ok(chunk)) = stream.next().await {
            for event in parser.feed(&chunk) {
                if let Some(id) = event.id.filter(|id| !id.is_empty()) {
                    *last_event_id.lock().await = Some(id);
                }
                if let Ok(message) = serde_json::from_str::<Message>(&event.data) {
                    if tx.send(message).is_err() {
                        return;
                    }
                }
            }
        }
    } else {
        let body = response.bytes().await.unwrap_or_default();
        if !body.is_empty() {
            if let Ok(message) = serde_json::from_slice::<Message>(&body) {
                let _ = tx.send(message);
            }
        }
    }
}
