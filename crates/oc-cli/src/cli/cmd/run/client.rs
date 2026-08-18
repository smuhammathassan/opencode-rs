//! Run-mode clients: an embedded HTTP client and a remote HTTP client for
//! `--attach`, mirroring `createOpencodeClient` from `@opencode-ai/sdk/v2`.

use futures::{future::BoxFuture, Stream, StreamExt};
use serde_json::{json, Value};

use super::types::{AgentSummary, GlobalEvent, ModelInput, PromptPart, SessionInfo};
use crate::cli::context::Context;

/// A stream of events from a running opencode server.
pub type EventStream = std::pin::Pin<Box<dyn Stream<Item = anyhow::Result<GlobalEvent>> + Send>>;

/// The client surface `opencode run` needs.
pub trait RunClient: Send + Sync {
    fn session_get(&self, id: String) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>>;
    fn session_list(&self) -> BoxFuture<'static, anyhow::Result<Vec<SessionInfo>>>;
    fn session_fork(&self, id: String) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>>;
    fn session_create(
        &self,
        title: Option<String>,
        agent: Option<String>,
        model: Option<ModelInput>,
        variant: Option<String>,
        permission: Vec<Value>,
    ) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>>;
    fn session_prompt(
        &self,
        session_id: String,
        agent: Option<String>,
        model: Option<ModelInput>,
        variant: Option<String>,
        parts: Vec<PromptPart>,
    ) -> BoxFuture<'static, anyhow::Result<()>>;
    fn session_command(
        &self,
        session_id: String,
        agent: Option<String>,
        model: Option<String>,
        command: String,
        arguments: String,
        variant: Option<String>,
    ) -> BoxFuture<'static, anyhow::Result<()>>;
    fn session_share(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, anyhow::Result<Option<String>>>;
    fn config_get(&self) -> BoxFuture<'static, anyhow::Result<Value>>;
    fn app_agents(&self) -> BoxFuture<'static, anyhow::Result<Vec<AgentSummary>>>;
    fn permission_reply(
        &self,
        request_id: String,
        reply: String,
    ) -> BoxFuture<'static, anyhow::Result<()>>;
    fn path_get(&self) -> BoxFuture<'static, anyhow::Result<Option<String>>>;
    fn subscribe(&self) -> BoxFuture<'static, anyhow::Result<EventStream>>;
}

/// In-process client backed by the embedded opencode server.
///
/// The CLI intentionally talks to the same HTTP surface used by `--attach`.
/// This keeps local and remote runs on one contract and ensures that route,
/// auth, SSE, and serialization regressions are exercised by both modes.
pub struct LocalClient {
    inner: AttachClient,
    // Keep the listener alive for the lifetime of the client. Dropping the
    // JoinHandle would otherwise stop the embedded server while a run is live.
    _listener: oc_server::server::Listener,
}

impl LocalClient {
    pub async fn create(ctx: &Context) -> anyhow::Result<Box<dyn RunClient>> {
        let mut options = oc_server::server::ListenOptions::new("127.0.0.1", 0);
        options.auth = oc_server::auth::AuthConfig::from_env();
        let listener = oc_server::server::listen(options).await?;
        let base_url = listener.url.to_string();
        let password = std::env::var("OPENCODE_SERVER_PASSWORD").ok();
        let username = std::env::var("OPENCODE_SERVER_USERNAME").ok();
        let inner = AttachClient::new(
            &base_url,
            Some(ctx.directory.to_string_lossy().into_owned()),
            password.as_deref(),
            username.as_deref(),
        );
        Ok(Box::new(Self {
            inner,
            _listener: listener,
        }))
    }
}

impl RunClient for LocalClient {
    fn session_get(&self, id: String) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>> {
        self.inner.session_get(id)
    }

    fn session_list(&self) -> BoxFuture<'static, anyhow::Result<Vec<SessionInfo>>> {
        self.inner.session_list()
    }

    fn session_fork(&self, id: String) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>> {
        self.inner.session_fork(id)
    }

    fn session_create(
        &self,
        title: Option<String>,
        agent: Option<String>,
        model: Option<ModelInput>,
        variant: Option<String>,
        permission: Vec<Value>,
    ) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>> {
        self.inner
            .session_create(title, agent, model, variant, permission)
    }

    fn session_prompt(
        &self,
        session_id: String,
        agent: Option<String>,
        model: Option<ModelInput>,
        variant: Option<String>,
        parts: Vec<PromptPart>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        self.inner
            .session_prompt(session_id, agent, model, variant, parts)
    }

    fn session_command(
        &self,
        session_id: String,
        agent: Option<String>,
        model: Option<String>,
        command: String,
        arguments: String,
        variant: Option<String>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        self.inner
            .session_command(session_id, agent, model, command, arguments, variant)
    }

    fn session_share(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, anyhow::Result<Option<String>>> {
        self.inner.session_share(session_id)
    }

    fn config_get(&self) -> BoxFuture<'static, anyhow::Result<Value>> {
        self.inner.config_get()
    }

    fn app_agents(&self) -> BoxFuture<'static, anyhow::Result<Vec<AgentSummary>>> {
        self.inner.app_agents()
    }

    fn permission_reply(
        &self,
        request_id: String,
        reply: String,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        self.inner.permission_reply(request_id, reply)
    }

    fn path_get(&self) -> BoxFuture<'static, anyhow::Result<Option<String>>> {
        self.inner.path_get()
    }

    fn subscribe(&self) -> BoxFuture<'static, anyhow::Result<EventStream>> {
        self.inner.subscribe()
    }
}

/// Remote client for `opencode run --attach <url>`.
pub struct AttachClient {
    base_url: String,
    directory: Option<String>,
    client: reqwest::Client,
}

fn auth_headers() -> Option<(String, String)> {
    let password = std::env::var("OPENCODE_SERVER_PASSWORD").ok()?;
    let username = std::env::var("OPENCODE_SERVER_USERNAME").unwrap_or_else(|_| "opencode".into());
    Some((username, password))
}

fn base64(input: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input.as_bytes())
}

fn urlencode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b':'
            | b'!'
            | b'*' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl AttachClient {
    /// Mirror `ServerAuth.headers({ password, username })` from
    /// reference/packages/opencode/src/server/auth.ts, falling back to
    /// `OPENCODE_SERVER_PASSWORD` / `OPENCODE_SERVER_USERNAME`.
    pub fn new(
        base_url: &str,
        directory: Option<String>,
        password: Option<&str>,
        username: Option<&str>,
    ) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        let auth = match (password, username) {
            (Some(password), _) => Some((
                username.unwrap_or("opencode").to_string(),
                password.to_string(),
            )),
            _ => auth_headers(),
        };
        if let Some((user, pass)) = auth {
            let value = format!("Basic {}", base64(&format!("{user}:{pass}")));
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&value) {
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
        }
        AttachClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            directory,
            client: reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .unwrap_or_default(),
        }
    }

    fn url(&self, path: &str) -> String {
        let mut url = format!("{}{}", self.base_url, path);
        if let Some(directory) = &self.directory {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{sep}directory={}", urlencode(directory));
        }
        url
    }
}

/// Extract the error message from a non-success response body. Handles the
/// v2 error shape (`{ "name": ..., "data": { "message": ... } }`) served by
/// oc-server and the v1 shape (`{ "error": { "message": ... } }`), falling
/// back to the HTTP status line.
fn extract_error_message(body: &str, status: reqwest::StatusCode) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("data")
                .and_then(|data| data.get("message"))
                .or_else(|| v.get("error").and_then(|error| error.get("message")))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_else(|| format!("HTTP {}", status))
}

async fn parse_response(response: reqwest::Response) -> anyhow::Result<Value> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow::anyhow!(extract_error_message(&body, status)));
    }
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body).map_err(|err| anyhow::anyhow!("invalid JSON response: {err}"))
}

fn unwrap_data(value: Value) -> Value {
    if let Some(data) = value.get("data") {
        data.clone()
    } else {
        value
    }
}

fn parse_session(value: Value) -> anyhow::Result<Option<SessionInfo>> {
    let value = unwrap_data(value);
    serde_json::from_value(value).map(Some).map_err(Into::into)
}

impl RunClient for AttachClient {
    fn session_get(&self, id: String) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>> {
        let client = self.client.clone();
        let url = self.url(&format!("/session/{id}"));
        Box::pin(async move {
            let response = client.get(&url).send().await?;
            parse_session(parse_response(response).await?)
        })
    }

    fn session_list(&self) -> BoxFuture<'static, anyhow::Result<Vec<SessionInfo>>> {
        let client = self.client.clone();
        let mut url = format!("{}/session?roots=true&limit=50", self.base_url);
        if let Some(directory) = &self.directory {
            url.push('&');
            url.push_str("directory=");
            url.push_str(&urlencode(directory));
        }
        Box::pin(async move {
            let response = client.get(&url).send().await?;
            let value = unwrap_data(parse_response(response).await?);
            let items = value
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect();
            Ok(items)
        })
    }

    fn session_fork(&self, id: String) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>> {
        let client = self.client.clone();
        let url = self.url(&format!("/session/{id}/fork"));
        Box::pin(async move {
            let response = client.post(&url).json(&json!({})).send().await?;
            parse_session(parse_response(response).await?)
        })
    }

    fn session_create(
        &self,
        title: Option<String>,
        agent: Option<String>,
        model: Option<ModelInput>,
        variant: Option<String>,
        permission: Vec<Value>,
    ) -> BoxFuture<'static, anyhow::Result<Option<SessionInfo>>> {
        let client = self.client.clone();
        let url = self.url("/session");
        Box::pin(async move {
            let mut body = serde_json::Map::new();
            if let Some(title) = title {
                body.insert("title".into(), title.into());
            }
            if let Some(agent) = agent {
                body.insert("agent".into(), agent.into());
            }
            if let Some(model) = model {
                let mut m = serde_json::Map::new();
                m.insert("id".into(), model.model_id.into());
                m.insert("providerID".into(), model.provider_id.into());
                if let Some(variant) = variant {
                    m.insert("variant".into(), variant.into());
                }
                body.insert("model".into(), m.into());
            }
            if !permission.is_empty() {
                body.insert("permission".into(), permission.into());
            }
            let response = client.post(&url).json(&Value::Object(body)).send().await?;
            parse_session(parse_response(response).await?)
        })
    }

    fn session_prompt(
        &self,
        session_id: String,
        agent: Option<String>,
        model: Option<ModelInput>,
        variant: Option<String>,
        parts: Vec<PromptPart>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        let client = self.client.clone();
        let url = self.url(&format!("/session/{session_id}/message"));
        Box::pin(async move {
            let mut body = serde_json::Map::new();
            if let Some(agent) = agent {
                body.insert("agent".into(), agent.into());
            }
            if let Some(model) = model {
                body.insert(
                    "model".into(),
                    json!({ "providerID": model.provider_id, "modelID": model.model_id }),
                );
            }
            if let Some(variant) = variant {
                body.insert("variant".into(), variant.into());
            }
            body.insert("parts".into(), serde_json::to_value(parts)?);
            let response = client.post(&url).json(&Value::Object(body)).send().await?;
            parse_response(response).await?;
            Ok(())
        })
    }

    fn session_command(
        &self,
        session_id: String,
        agent: Option<String>,
        model: Option<String>,
        command: String,
        arguments: String,
        variant: Option<String>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        let client = self.client.clone();
        let url = self.url(&format!("/session/{session_id}/command"));
        Box::pin(async move {
            let mut body = serde_json::Map::new();
            if let Some(agent) = agent {
                body.insert("agent".into(), agent.into());
            }
            if let Some(model) = model {
                body.insert("model".into(), model.into());
            }
            if let Some(variant) = variant {
                body.insert("variant".into(), variant.into());
            }
            body.insert("command".into(), command.into());
            body.insert("arguments".into(), arguments.into());
            let response = client.post(&url).json(&Value::Object(body)).send().await?;
            parse_response(response).await?;
            Ok(())
        })
    }

    fn session_share(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, anyhow::Result<Option<String>>> {
        let client = self.client.clone();
        let url = self.url(&format!("/session/{session_id}/share"));
        Box::pin(async move {
            let response = client.post(&url).json(&json!({})).send().await?;
            let value = unwrap_data(parse_response(response).await?);
            Ok(value.get("url").and_then(Value::as_str).map(String::from))
        })
    }

    fn config_get(&self) -> BoxFuture<'static, anyhow::Result<Value>> {
        let client = self.client.clone();
        let url = self.url("/config");
        Box::pin(async move {
            let response = client.get(&url).send().await?;
            Ok(unwrap_data(parse_response(response).await?))
        })
    }

    fn app_agents(&self) -> BoxFuture<'static, anyhow::Result<Vec<AgentSummary>>> {
        let client = self.client.clone();
        // The v1 instance route is `/agent`; `/app/agents` is not part of the
        // HTTP contract served by oc-server.
        let url = self.url("/agent");
        Box::pin(async move {
            let response = client.get(&url).send().await?;
            let value = unwrap_data(parse_response(response).await?);
            let items = value
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect();
            Ok(items)
        })
    }

    fn permission_reply(
        &self,
        request_id: String,
        reply: String,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        let client = self.client.clone();
        let url = self.url(&format!("/permission/{request_id}/reply"));
        Box::pin(async move {
            let response = client
                .post(&url)
                .json(&json!({ "reply": reply }))
                .send()
                .await?;
            parse_response(response).await?;
            Ok(())
        })
    }

    fn path_get(&self) -> BoxFuture<'static, anyhow::Result<Option<String>>> {
        let client = self.client.clone();
        let url = self.url("/path");
        Box::pin(async move {
            let response = client.get(&url).send().await?;
            let value = unwrap_data(parse_response(response).await?);
            Ok(value
                .get("directory")
                .and_then(Value::as_str)
                .map(String::from))
        })
    }

    fn subscribe(&self) -> BoxFuture<'static, anyhow::Result<EventStream>> {
        let client = self.client.clone();
        let mut url = format!("{}/event", self.base_url);
        if let Some(directory) = &self.directory {
            url.push('?');
            url.push_str("directory=");
            url.push_str(&urlencode(directory));
        }
        Box::pin(async move {
            let response = client.get(&url).send().await?.error_for_status()?;
            let bytes = response.bytes_stream();
            let stream: EventStream = Box::pin(sse_stream(bytes));
            Ok(stream)
        })
    }
}

fn sse_stream<S>(stream: S) -> impl Stream<Item = anyhow::Result<GlobalEvent>>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    futures::stream::unfold(stream, |mut stream| async move {
        let mut data = String::new();
        let mut buffer: Vec<u8> = Vec::new();
        let mut in_event = false;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(err) => return Some((Err(anyhow::Error::new(err)), stream)),
            };
            buffer.extend_from_slice(&chunk);
            while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line =
                    String::from_utf8_lossy(&line[..line.len().saturating_sub(1)]).to_string();
                if line.trim().is_empty() {
                    if in_event {
                        let json = std::mem::take(&mut data);
                        let event: GlobalEvent = match serde_json::from_str(&json) {
                            Ok(event) => event,
                            Err(err) => {
                                return Some((
                                    Err(anyhow::anyhow!("invalid event: {err}")),
                                    stream,
                                ));
                            }
                        };
                        return Some((Ok(event), stream));
                    }
                    continue;
                }
                if let Some(value) = line.strip_prefix("data:") {
                    in_event = true;
                    data.push_str(value.trim_start());
                    data.push('\n');
                }
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encodes_slashes() {
        assert_eq!(urlencode("/a/b c"), "%2Fa%2Fb%20c");
    }

    #[test]
    fn unwraps_data_envelope() {
        let value = serde_json::json!({ "data": { "id": "x" } });
        assert_eq!(
            unwrap_data(value).get("id").and_then(Value::as_str),
            Some("x")
        );
    }

    #[test]
    fn error_response_extracts_v2_data_message() {
        // v2 error shape served by oc-server: `{ name, data: { message } }`.
        let body = serde_json::json!({
            "name": "NotFoundError",
            "data": { "message": "Command not found: help" }
        })
        .to_string();
        assert_eq!(
            extract_error_message(&body, reqwest::StatusCode::NOT_FOUND),
            "Command not found: help"
        );
    }

    #[test]
    fn error_response_extracts_v1_error_message() {
        let body = serde_json::json!({ "error": { "message": "Session not found" } }).to_string();
        assert_eq!(
            extract_error_message(&body, reqwest::StatusCode::BAD_REQUEST),
            "Session not found"
        );
    }

    #[test]
    fn error_response_falls_back_to_http_status() {
        assert_eq!(
            extract_error_message("", reqwest::StatusCode::NOT_FOUND),
            "HTTP 404 Not Found"
        );
    }
}
