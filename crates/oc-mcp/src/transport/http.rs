//! Streamable HTTP client transport.
//!
//! From `@modelcontextprotocol/sdk@1.29.0` `client/streamableHttp.js` (with the
//! opencode session-recovery patch), as configured by
//! `reference/packages/opencode/src/mcp/index.ts` (`connectRemote`,
//! `startAuth`).

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

pub struct StreamableHTTPClientTransport {
    url: Url,
    headers: HashMap<String, String>,
    auth_provider: Option<Arc<dyn OAuthClientProvider>>,
    auth_client: Arc<crate::oauth::AuthClient>,
    http: HttpClient,
    session_id: Arc<Mutex<Option<String>>>,
    protocol_version: Arc<Mutex<Option<String>>>,
    has_completed_auth_flow: Arc<AtomicBool>,
    tx: Mutex<Option<mpsc::UnboundedSender<Message>>>,
    open: OpenFlag,
}

impl StreamableHTTPClientTransport {
    pub fn new(
        url: Url,
        headers: Option<HashMap<String, String>>,
        auth_provider: Option<Arc<dyn OAuthClientProvider>>,
    ) -> Self {
        StreamableHTTPClientTransport {
            url,
            headers: headers.unwrap_or_default(),
            auth_provider,
            auth_client: Arc::new(crate::oauth::AuthClient::new()),
            http: crate::oauth::http_client(),
            session_id: Arc::new(Mutex::new(None)),
            protocol_version: Arc::new(Mutex::new(None)),
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
        if let Some(version) = self.protocol_version.lock().await.as_deref() {
            if let Ok(value) = HeaderValue::from_str(version) {
                headers.insert("mcp-protocol-version", value);
            }
        }
        if let Some(session_id) = self.session_id.lock().await.as_deref() {
            if let Ok(value) = HeaderValue::from_str(session_id) {
                headers.insert("mcp-session-id", value);
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

    /// Run the OAuth flow when the server responds 401/403. Returns `Ok(())`
    /// when a retry should happen; `Err(Error::Unauthorized)` when interactive
    /// authorization is required.
    async fn handle_auth(
        &self,
        resource_metadata_url: Option<Url>,
        scope: Option<String>,
    ) -> Result<()> {
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
            scope,
            resource_metadata_url,
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

    async fn open_stream(&self) -> Result<reqwest::Response> {
        let headers = self.common_headers().await?;
        let response = self
            .http
            .get(self.url.clone())
            .headers(headers)
            .header(ACCEPT, "application/json, text/event-stream")
            .send()
            .await?;

        self.capture_session_id(&response).await;

        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                let (resource_metadata_url, scope) = challenge_from(&response);
                self.handle_auth(resource_metadata_url, scope).await?;
                let headers = self.common_headers().await?;
                let retry = self
                    .http
                    .get(self.url.clone())
                    .headers(headers)
                    .header(ACCEPT, "application/json, text/event-stream")
                    .send()
                    .await?;
                if retry.status().is_success() {
                    self.capture_session_id(&retry).await;
                    return Ok(retry);
                }
                let retry_status = retry.status();
                let text = retry.text().await.unwrap_or_default();
                return Err(crate::Error::message(format!(
                    "Error opening MCP stream: {retry_status} {text}"
                )));
            }
            let text = response.text().await.unwrap_or_default();
            return Err(crate::Error::message(format!(
                "Error opening MCP stream: {status} {text}"
            )));
        }
        Ok(response)
    }

    async fn capture_session_id(&self, response: &reqwest::Response) {
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            *self.session_id.lock().await = Some(session_id.to_string());
        }
    }
}

impl Transport for StreamableHTTPClientTransport {
    fn start(&self) -> BoxFuture<'_, Result<MessageReceiver>> {
        Box::pin(async move {
            let (tx, rx) = mpsc::unbounded_channel();
            let response = self.open_stream().await?;
            *self.tx.lock().await = Some(tx.clone());

            let url = self.url.clone();
            let session_id = self.session_id.clone();
            let protocol_version = self.protocol_version.clone();
            let http = self.http.clone();
            let auth_provider = self.auth_provider.clone();
            let auth_client = self.auth_client.clone();
            let headers = self.headers.clone();
            let open = self.open.clone();

            tokio::spawn(async move {
                let mut response: Option<reqwest::Response> = Some(response);
                loop {
                    if open.is_closed() {
                        break;
                    }
                    let Some(current) = response.take() else {
                        match open_stream_again(
                            &url,
                            &headers,
                            &http,
                            &auth_provider,
                            &auth_client,
                            &session_id,
                            &protocol_version,
                        )
                        .await
                        {
                            Ok(new_response) => response = Some(new_response),
                            Err(error) => {
                                if matches!(error, crate::Error::Unauthorized { .. }) {
                                    debug!("MCP stream requires authorization, giving up");
                                    break;
                                }
                                debug!("MCP stream reconnect failed: {error}");
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }
                        continue;
                    };
                    let done = consume_stream(current, &tx).await;
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
            let mut headers = self.common_headers().await?;
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(
                ACCEPT,
                HeaderValue::from_static("application/json, text/event-stream"),
            );
            let body = serde_json::to_string(&message)?;

            let response = self
                .http
                .post(self.url.clone())
                .headers(headers)
                .body(body)
                .send()
                .await?;

            self.capture_session_id(&response).await;

            if !response.status().is_success() {
                let status = response.status();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    let challenge = www_authenticate_params(&response);
                    let resource_metadata_url = challenge.0.and_then(|url| Url::parse(&url).ok());
                    match self.handle_auth(resource_metadata_url, challenge.1).await {
                        Ok(()) => return self.send(message).await,
                        Err(error) => return Err(error),
                    }
                }
                let text = response.text().await.unwrap_or_default();
                if status.as_u16() == 404 && self.session_id.lock().await.is_some() {
                    // TODO(integration): re-run the initialize handshake like
                    // the SDK patch's `onsessionexpired`/`_recoverSession` path.
                    *self.session_id.lock().await = None;
                }
                return Err(crate::Error::message(format!(
                    "MCP server returned {status}: {text}"
                )));
            }

            if let Some(tx) = self.tx.lock().await.as_ref() {
                deliver_response(response, tx).await;
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

    fn set_protocol_version(&self, version: String) -> BoxFuture<'_, ()> {
        let protocol_version = self.protocol_version.clone();
        Box::pin(async move {
            *protocol_version.lock().await = Some(version);
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

fn challenge_from(response: &reqwest::Response) -> (Option<Url>, Option<String>) {
    let (resource, scope) = www_authenticate_params(response);
    (resource.and_then(|url| Url::parse(&url).ok()), scope)
}

fn www_authenticate_params(response: &reqwest::Response) -> (Option<String>, Option<String>) {
    let header = response
        .headers()
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let (resource, scope, _error) =
        crate::oauth::extract_www_authenticate_params(header).unwrap_or((None, None, None));
    (resource, scope)
}

async fn consume_stream(response: reqwest::Response, tx: &mpsc::UnboundedSender<Message>) -> bool {
    let mut stream = response.bytes_stream();
    let mut parser = SseParser::new();
    loop {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    if let Some(message) = parse_sse_message(event.data.as_str()) {
                        if tx.send(message).is_err() {
                            return true;
                        }
                    }
                }
            }
            Ok(Some(Err(error))) => {
                debug!("MCP stream read error: {error}");
                return false;
            }
            Ok(None) => return false,
            Err(_) => {}
        }
    }
}

fn parse_sse_message(data: &str) -> Option<Message> {
    serde_json::from_str::<Message>(data).ok()
}

async fn open_stream_again(
    url: &Url,
    headers: &HashMap<String, String>,
    http: &HttpClient,
    auth_provider: &Option<Arc<dyn OAuthClientProvider>>,
    _auth_client: &Arc<crate::oauth::AuthClient>,
    session_id: &Arc<Mutex<Option<String>>>,
    protocol_version: &Arc<Mutex<Option<String>>>,
) -> Result<reqwest::Response> {
    let mut header_map = HeaderMap::new();
    if let Some(provider) = auth_provider {
        if let Some(tokens) = provider.tokens().await? {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", tokens.access_token)) {
                header_map.insert(AUTHORIZATION, value);
            }
        }
    }
    if let Some(version) = protocol_version.lock().await.as_deref() {
        if let Ok(value) = HeaderValue::from_str(version) {
            header_map.insert("mcp-protocol-version", value);
        }
    }
    if let Some(sid) = session_id.lock().await.as_deref() {
        if let Ok(value) = HeaderValue::from_str(sid) {
            header_map.insert("mcp-session-id", value);
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
    let response = http
        .get(url.clone())
        .headers(header_map)
        .header(ACCEPT, "application/json, text/event-stream")
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(crate::Error::unauthorized("authorization required"));
        }
        return Err(crate::Error::message(format!(
            "MCP stream reconnect failed: {status}"
        )));
    }
    if let Some(new_session_id) = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
    {
        *session_id.lock().await = Some(new_session_id.to_string());
    }
    Ok(response)
}

async fn deliver_response(response: reqwest::Response, tx: &mpsc::UnboundedSender<Message>) {
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
                if let Some(message) = parse_sse_message(event.data.as_str()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_message() {
        let message = parse_sse_message(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        assert!(message.is_some());
        assert_eq!(parse_sse_message("not json"), None);
    }
}
