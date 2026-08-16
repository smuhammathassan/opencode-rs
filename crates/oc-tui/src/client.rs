//! Server client (HTTP + SSE).
//!
//! Builds against the intended `oc-client` API surface (the endpoints the TUI
//! exercises, per `reference/packages/sdk/js/src/v2/gen/sdk.gen.ts`). The
//! HTTP/SSE transport is implemented directly here with `reqwest` until
//! `oc-client` exposes a typed client.
//!
//! TODO(integration): once `oc-client` lands, replace `HttpSdkClient` with a
//! thin adapter over it while keeping the `SdkClient` trait.

use std::collections::HashMap;
use std::pin::Pin;

use anyhow::{anyhow, bail, Context, Result};
use futures::stream::BoxStream;
use futures::StreamExt;

use crate::types::*;

pub struct ClientConfig {
    pub url: String,
    pub directory: Option<String>,
    pub workspace: Option<String>,
}

/// A server-originated TUI control request delivered through the v1 control
/// queue. The queue is used by remote clients that cannot inject terminal
/// keystrokes directly.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct TuiControlRequest {
    pub path: String,
    #[serde(default)]
    pub body: serde_json::Value,
}

/// The client surface the TUI relies on.
pub trait SdkClient: Send + Sync {
    fn subscribe_events(&self) -> Result<BoxStream<'static, GlobalEvent>>;

    // Sessions
    fn session_list(&self) -> BoxFuture<Result<Vec<Session>>>;
    fn session_get(&self, session_id: &str) -> BoxFuture<Result<Session>>;
    fn session_messages(&self, session_id: &str) -> BoxFuture<Result<Vec<SessionMessageData>>>;
    fn session_todo(&self, session_id: &str) -> BoxFuture<Result<Vec<Todo>>>;
    fn session_diff(&self, session_id: &str) -> BoxFuture<Result<Vec<SnapshotFileDiff>>>;
    fn session_create(&self, input: SessionCreateInput) -> BoxFuture<Result<Session>>;
    fn session_prompt(&self, input: PromptInput) -> BoxFuture<Result<()>>;
    fn session_command(&self, input: CommandInput) -> BoxFuture<Result<()>>;
    fn session_shell(&self, input: ShellInput) -> BoxFuture<Result<()>>;
    fn session_abort(&self, session_id: &str) -> BoxFuture<Result<()>>;
    fn session_compact(&self, session_id: &str) -> BoxFuture<Result<()>>;
    fn session_delete(&self, session_id: &str) -> BoxFuture<Result<()>>;
    fn session_revert(&self, session_id: &str, message_id: &str) -> BoxFuture<Result<()>>;
    fn session_unrevert(&self, session_id: &str) -> BoxFuture<Result<()>>;
    fn session_fork(&self, session_id: &str) -> BoxFuture<Result<Option<Session>>>;
    fn session_share(&self, session_id: &str) -> BoxFuture<Result<()>>;
    fn session_unshare(&self, session_id: &str) -> BoxFuture<Result<()>>;
    fn session_summarize(
        &self,
        session_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> BoxFuture<Result<()>>;

    // Experimental background session jobs.
    fn experimental_session_background_list(&self) -> BoxFuture<Result<Vec<BackgroundJobInfo>>>;
    fn experimental_session_background_status(
        &self,
        session_id: &str,
    ) -> BoxFuture<Result<BackgroundJobInfo>>;
    fn experimental_session_background_cancel(
        &self,
        session_id: &str,
    ) -> BoxFuture<Result<BackgroundJobInfo>>;

    // Status & configuration
    fn session_status(&self) -> BoxFuture<Result<HashMap<String, SessionStatus>>>;
    fn config_providers(&self) -> BoxFuture<Result<ConfigProviders>>;
    fn provider_list(&self) -> BoxFuture<Result<ProviderList>>;
    fn app_agents(&self) -> BoxFuture<Result<Vec<Agent>>>;
    fn skill_list(&self) -> BoxFuture<Result<Vec<Skill>>>;
    fn config_get(&self) -> BoxFuture<Result<Config>>;
    fn command_list(&self) -> BoxFuture<Result<Vec<Command>>>;
    fn experimental_capabilities(&self) -> BoxFuture<Result<ExperimentalCapabilities>>;
    fn experimental_console(&self) -> BoxFuture<Result<ConsoleState>>;

    // Permissions & questions
    fn permission_reply(
        &self,
        request_id: &str,
        reply: &str,
        message: Option<&str>,
    ) -> BoxFuture<Result<()>>;
    fn question_reply(&self, request_id: &str, answers: Vec<Vec<String>>) -> BoxFuture<Result<()>>;
    fn question_reject(&self, request_id: &str) -> BoxFuture<Result<()>>;

    // Filesystem
    fn fs_find(&self, query: &str, limit: &str) -> BoxFuture<Result<Vec<FileSystemEntry>>>;

    // Remote TUI control queue.
    fn tui_control_next(&self) -> BoxFuture<Result<TuiControlRequest>>;
    fn tui_control_response(&self, body: serde_json::Value) -> BoxFuture<Result<()>>;
}

pub type BoxFuture<T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'static>>;

#[derive(Debug, Clone, Default)]
pub struct SessionCreateInput {
    pub directory: Option<String>,
    pub workspace: Option<String>,
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptInput {
    pub session_id: String,
    pub agent: String,
    pub model: ModelRef,
    pub variant: Option<String>,
    pub parts: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct CommandInput {
    pub session_id: String,
    pub command: String,
    pub arguments: String,
    pub agent: String,
    pub model: String,
    pub variant: Option<String>,
    pub parts: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ShellInput {
    pub session_id: String,
    pub agent: String,
    pub model: ModelRef,
    pub command: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProviders {
    pub providers: Vec<Provider>,
    pub default: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderList {
    pub all: Vec<Provider>,
    pub default: HashMap<String, String>,
    pub connected: Vec<String>,
}

/// HTTP implementation of `SdkClient` speaking the opencode server protocol.
pub struct HttpSdkClient {
    http: reqwest::Client,
    url: String,
    directory: Option<String>,
    workspace: Option<String>,
}

impl HttpSdkClient {
    pub fn new(config: ClientConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .build()
            .context("failed to build http client")?;
        Ok(HttpSdkClient {
            http,
            url: config.url.trim_end_matches('/').to_string(),
            directory: config.directory,
            workspace: config.workspace,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.url, path);
        let mut req = self.http.request(method, url);
        if let Some(password) = std::env::var_os("OPENCODE_SERVER_PASSWORD") {
            let username = std::env::var("OPENCODE_SERVER_USERNAME")
                .unwrap_or_else(|_| "opencode".to_string());
            req = req.basic_auth(username, Some(password.to_string_lossy().into_owned()));
        }
        if let Some(directory) = &self.directory {
            req = req.query(&[("directory", directory)]);
        }
        if let Some(workspace) = &self.workspace {
            req = req.query(&[("workspace", workspace)]);
        }
        req
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .request(reqwest::Method::GET, path)
            .send()
            .await
            .with_context(|| format!("GET {path} failed"))?;
        self.decode(response, path).await
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let response = self
            .request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {path} failed"))?;
        self.decode(response, path).await
    }

    async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
        path: &str,
    ) -> Result<T> {
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .context("failed to read response body")?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            bail!("{path} returned {status}: {text}");
        }
        serde_json::from_slice(&bytes).with_context(|| format!("failed to decode {path} response"))
    }
}

impl SdkClient for HttpSdkClient {
    fn subscribe_events(&self) -> Result<BoxStream<'static, GlobalEvent>> {
        let this = self.clone_ref();
        let (tx, rx) = tokio::sync::mpsc::channel::<GlobalEvent>(256);
        // Background task: connect to the event stream and reconnect with
        // exponential backoff. Mirrors the reference SSE loop in
        // reference/packages/tui/src/context/sdk.tsx (`startSSE`).
        tokio::spawn(async move {
            let mut attempt = 0u32;
            loop {
                match this.stream_events_once(&tx).await {
                    Ok(()) => {}
                    Err(error) => {
                        tracing::debug!(%error, "event stream ended, reconnecting");
                    }
                }
                if tx.is_closed() {
                    break;
                }
                attempt += 1;
                let backoff = (1000u64 * 2u64.pow(attempt.saturating_sub(1).min(5))).min(30_000);
                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
            }
        });
        Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })))
    }

    fn session_list(&self) -> BoxFuture<Result<Vec<Session>>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/session").await })
    }

    fn session_get(&self, session_id: &str) -> BoxFuture<Result<Session>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move { this.get_json(&format!("/session/{session_id}")).await })
    }

    fn session_messages(&self, session_id: &str) -> BoxFuture<Result<Vec<SessionMessageData>>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            this.get_json(&format!("/session/{session_id}/message?limit=100"))
                .await
        })
    }

    fn session_todo(&self, session_id: &str) -> BoxFuture<Result<Vec<Todo>>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move { this.get_json(&format!("/session/{session_id}/todo")).await })
    }

    fn session_diff(&self, session_id: &str) -> BoxFuture<Result<Vec<SnapshotFileDiff>>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move { this.get_json(&format!("/session/{session_id}/diff")).await })
    }

    fn session_create(&self, input: SessionCreateInput) -> BoxFuture<Result<Session>> {
        let this = self.clone_ref();
        Box::pin(async move {
            let mut body = serde_json::Map::new();
            if let Some(agent) = &input.agent {
                body.insert("agent".into(), serde_json::json!(agent));
            }
            if let Some(model) = &input.model {
                body.insert("model".into(), serde_json::to_value(model)?);
            }
            if let Some(workspace_id) = &input.workspace_id {
                body.insert("workspaceID".into(), serde_json::json!(workspace_id));
            }
            this.post_json("/session", &serde_json::Value::Object(body))
                .await
        })
    }

    fn session_prompt(&self, input: PromptInput) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        Box::pin(async move {
            let mut body = serde_json::Map::new();
            body.insert("agent".into(), serde_json::json!(input.agent));
            body.insert(
                "model".into(),
                serde_json::json!({ "providerID": input.model.provider_id, "modelID": input.model.id }),
            );
            if let Some(variant) = &input.variant {
                body.insert("variant".into(), serde_json::json!(variant));
            }
            body.insert("parts".into(), serde_json::Value::Array(input.parts));
            let _: serde_json::Value = this
                .post_json(
                    &format!("/session/{}/message", input.session_id),
                    &serde_json::Value::Object(body),
                )
                .await?;
            Ok(())
        })
    }

    fn session_command(&self, input: CommandInput) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        Box::pin(async move {
            let mut body = serde_json::Map::new();
            body.insert("command".into(), serde_json::json!(input.command));
            body.insert("arguments".into(), serde_json::json!(input.arguments));
            body.insert("agent".into(), serde_json::json!(input.agent));
            body.insert("model".into(), serde_json::json!(input.model));
            if let Some(variant) = &input.variant {
                body.insert("variant".into(), serde_json::json!(variant));
            }
            body.insert("parts".into(), serde_json::Value::Array(input.parts));
            let _: serde_json::Value = this
                .post_json(
                    &format!("/session/{}/command", input.session_id),
                    &serde_json::Value::Object(body),
                )
                .await?;
            Ok(())
        })
    }

    fn session_shell(&self, input: ShellInput) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        Box::pin(async move {
            let body = serde_json::json!({
                "agent": input.agent,
                "model": { "providerID": input.model.provider_id, "modelID": input.model.id },
                "command": input.command,
            });
            let _: serde_json::Value = this
                .post_json(&format!("/session/{}/shell", input.session_id), &body)
                .await?;
            Ok(())
        })
    }

    fn session_abort(&self, session_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let _: serde_json::Value = this
                .post_json(
                    &format!("/session/{session_id}/abort"),
                    &serde_json::Value::Null,
                )
                .await?;
            Ok(())
        })
    }
    fn session_compact(&self, session_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let _: serde_json::Value = this
                .post_json(
                    &format!("/session/{session_id}/compact"),
                    &serde_json::Value::Null,
                )
                .await?;
            Ok(())
        })
    }

    fn session_delete(&self, session_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let response = this
                .request(reqwest::Method::DELETE, &format!("/session/{session_id}"))
                .send()
                .await?;
            if !response.status().is_success() {
                bail!("delete session returned {}", response.status());
            }
            Ok(())
        })
    }

    fn session_revert(&self, session_id: &str, message_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let (session_id, message_id) = (session_id.to_string(), message_id.to_string());
        Box::pin(async move {
            let body = serde_json::json!({ "messageID": message_id });
            let _: serde_json::Value = this
                .post_json(&format!("/session/{session_id}/revert"), &body)
                .await?;
            Ok(())
        })
    }

    fn session_unrevert(&self, session_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let _: serde_json::Value = this
                .post_json(
                    &format!("/session/{session_id}/unrevert"),
                    &serde_json::Value::Null,
                )
                .await?;
            Ok(())
        })
    }

    fn session_fork(&self, session_id: &str) -> BoxFuture<Result<Option<Session>>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let result: serde_json::Value = this
                .post_json(
                    &format!("/session/{session_id}/fork"),
                    &serde_json::Value::Null,
                )
                .await?;
            serde_json::from_value(result).map_err(Into::into)
        })
    }

    fn session_share(&self, session_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let _: serde_json::Value = this
                .post_json(
                    &format!("/session/{session_id}/share"),
                    &serde_json::Value::Null,
                )
                .await?;
            Ok(())
        })
    }

    fn session_unshare(&self, session_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let response = this
                .request(
                    reqwest::Method::DELETE,
                    &format!("/session/{session_id}/share"),
                )
                .send()
                .await?;
            if !response.status().is_success() {
                bail!("unshare returned {}", response.status());
            }
            Ok(())
        })
    }

    fn session_summarize(
        &self,
        session_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let (session_id, provider_id, model_id) = (
            session_id.to_string(),
            provider_id.to_string(),
            model_id.to_string(),
        );
        Box::pin(async move {
            let body = serde_json::json!({ "providerID": provider_id, "modelID": model_id });
            let _: serde_json::Value = this
                .post_json(&format!("/session/{session_id}/summarize"), &body)
                .await?;
            Ok(())
        })
    }

    fn experimental_session_background_list(&self) -> BoxFuture<Result<Vec<BackgroundJobInfo>>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/experimental/session/background").await })
    }

    fn experimental_session_background_status(
        &self,
        session_id: &str,
    ) -> BoxFuture<Result<BackgroundJobInfo>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            this.get_json(&format!("/experimental/session/{session_id}/background"))
                .await
        })
    }

    fn experimental_session_background_cancel(
        &self,
        session_id: &str,
    ) -> BoxFuture<Result<BackgroundJobInfo>> {
        let this = self.clone_ref();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let response = this
                .request(
                    reqwest::Method::DELETE,
                    &format!("/experimental/session/{session_id}/background"),
                )
                .send()
                .await
                .with_context(|| {
                    format!("DELETE /experimental/session/{session_id}/background failed")
                })?;
            this.decode(
                response,
                &format!("/experimental/session/{session_id}/background"),
            )
            .await
        })
    }

    fn session_status(&self) -> BoxFuture<Result<HashMap<String, SessionStatus>>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/session/status").await })
    }

    fn config_providers(&self) -> BoxFuture<Result<ConfigProviders>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/config/providers").await })
    }

    fn provider_list(&self) -> BoxFuture<Result<ProviderList>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/provider").await })
    }

    fn app_agents(&self) -> BoxFuture<Result<Vec<Agent>>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/agent").await })
    }

    fn skill_list(&self) -> BoxFuture<Result<Vec<Skill>>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/skill").await })
    }

    fn config_get(&self) -> BoxFuture<Result<Config>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/config").await })
    }

    fn command_list(&self) -> BoxFuture<Result<Vec<Command>>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/command").await })
    }

    fn experimental_capabilities(&self) -> BoxFuture<Result<ExperimentalCapabilities>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/experimental/capabilities").await })
    }

    fn experimental_console(&self) -> BoxFuture<Result<ConsoleState>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/experimental/console").await })
    }

    fn permission_reply(
        &self,
        request_id: &str,
        reply: &str,
        message: Option<&str>,
    ) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let (request_id, reply) = (request_id.to_string(), reply.to_string());
        let message = message.map(str::to_string);
        Box::pin(async move {
            let mut body = serde_json::Map::new();
            body.insert("reply".into(), serde_json::json!(reply));
            if let Some(message) = &message {
                body.insert("message".into(), serde_json::json!(message));
            }
            let _: serde_json::Value = this
                .post_json(
                    &format!("/permission/{request_id}/reply"),
                    &serde_json::Value::Object(body),
                )
                .await?;
            Ok(())
        })
    }

    fn question_reply(&self, request_id: &str, answers: Vec<Vec<String>>) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let request_id = request_id.to_string();
        Box::pin(async move {
            let body = serde_json::json!({ "answers": answers });
            let _: serde_json::Value = this
                .post_json(&format!("/question/{request_id}/reply"), &body)
                .await?;
            Ok(())
        })
    }

    fn question_reject(&self, request_id: &str) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        let request_id = request_id.to_string();
        Box::pin(async move {
            let _: serde_json::Value = this
                .post_json(
                    &format!("/question/{request_id}/reject"),
                    &serde_json::Value::Null,
                )
                .await?;
            Ok(())
        })
    }

    fn fs_find(&self, query: &str, limit: &str) -> BoxFuture<Result<Vec<FileSystemEntry>>> {
        let this = self.clone_ref();
        let (query, limit) = (query.to_string(), limit.to_string());
        Box::pin(async move {
            let mut req = this.request(reqwest::Method::GET, "/find");
            req = req.query(&[("query", &query), ("limit", &limit)]);
            let response = req.send().await?;
            let bytes = response.bytes().await?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            Ok(value
                .get("data")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect())
        })
    }

    fn tui_control_next(&self) -> BoxFuture<Result<TuiControlRequest>> {
        let this = self.clone_ref();
        Box::pin(async move { this.get_json("/tui/control/next").await })
    }

    fn tui_control_response(&self, body: serde_json::Value) -> BoxFuture<Result<()>> {
        let this = self.clone_ref();
        Box::pin(async move {
            let _: serde_json::Value = this.post_json("/tui/control/response", &body).await?;
            Ok(())
        })
    }
}

impl HttpSdkClient {
    fn clone_ref(&self) -> Arc<HttpSdkClient> {
        Arc::new(HttpSdkClient {
            http: self.http.clone(),
            url: self.url.clone(),
            directory: self.directory.clone(),
            workspace: self.workspace.clone(),
        })
    }

    /// Stream parsed events from a single `/global/event` connection.
    async fn stream_events_once(&self, tx: &tokio::sync::mpsc::Sender<GlobalEvent>) -> Result<()> {
        let mut req = self.request(reqwest::Method::GET, "/global/event");
        req = req.header("accept", "text/event-stream");
        let response = req.send().await.context("event stream connect failed")?;
        if !response.status().is_success() {
            bail!("event stream returned {}", response.status());
        }
        let mut bytes = response.bytes_stream();
        let mut parser = SseParser::default();
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.context("event stream read failed")?;
            let text = String::from_utf8_lossy(&chunk);
            for line in text.split('\n') {
                if let Some(event) = parser.push_line(line) {
                    if tx.send(event).await.is_err() {
                        return Err(anyhow!("event receiver closed"));
                    }
                }
            }
        }
        if let Some(event) = parser.finish_event() {
            let _ = tx.send(event).await;
        }
        Ok(())
    }
}

use std::sync::Arc;

/// Minimal SSE parser: accumulates `data:` payloads and yields one event per
/// blank line.
struct SseParser {
    data: String,
}

impl Default for SseParser {
    fn default() -> Self {
        SseParser {
            data: String::new(),
        }
    }
}

impl SseParser {
    fn push_line(&mut self, line: &str) -> Option<GlobalEvent> {
        if line.is_empty() {
            return self.finish_event();
        }
        if let Some(value) = line.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(value);
        }
        // Comments and other fields are ignored.
        None
    }

    fn finish_event(&mut self) -> Option<GlobalEvent> {
        if self.data.is_empty() {
            return None;
        }
        let data = std::mem::take(&mut self.data);
        match serde_json::from_str::<GlobalEvent>(&data) {
            Ok(event) => {
                if event.payload.r#type == "sync" {
                    // Sync events are internal and filtered by the event bus.
                    return None;
                }
                Some(event)
            }
            Err(error) => {
                tracing::debug!(%error, "failed to parse event payload");
                None
            }
        }
    }
}

/// Provide a mock client for headless tests.
pub struct MockSdkClient {
    pub events: std::sync::Mutex<Vec<GlobalEvent>>,
    pub background_jobs: std::sync::Mutex<Vec<BackgroundJobInfo>>,
    pub cancelled_background_jobs: std::sync::Mutex<Vec<String>>,
}

impl MockSdkClient {
    pub fn new() -> Self {
        MockSdkClient {
            events: std::sync::Mutex::new(Vec::new()),
            background_jobs: std::sync::Mutex::new(Vec::new()),
            cancelled_background_jobs: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for MockSdkClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SdkClient for MockSdkClient {
    fn subscribe_events(&self) -> Result<BoxStream<'static, GlobalEvent>> {
        let events: Vec<GlobalEvent> = self.events.lock().unwrap().clone();
        Ok(futures::stream::iter(events).boxed())
    }
    fn session_list(&self) -> BoxFuture<Result<Vec<Session>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn session_get(&self, _session_id: &str) -> BoxFuture<Result<Session>> {
        Box::pin(async { Err(anyhow!("mock: session not found")) })
    }
    fn session_messages(&self, _session_id: &str) -> BoxFuture<Result<Vec<SessionMessageData>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn session_todo(&self, _session_id: &str) -> BoxFuture<Result<Vec<Todo>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn session_diff(&self, _session_id: &str) -> BoxFuture<Result<Vec<SnapshotFileDiff>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn session_create(&self, _input: SessionCreateInput) -> BoxFuture<Result<Session>> {
        Box::pin(async { Err(anyhow!("mock")) })
    }
    fn session_prompt(&self, _input: PromptInput) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_command(&self, _input: CommandInput) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_shell(&self, _input: ShellInput) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_abort(&self, _session_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_compact(&self, _session_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_delete(&self, _session_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_revert(&self, _session_id: &str, _message_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_unrevert(&self, _session_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_fork(&self, _session_id: &str) -> BoxFuture<Result<Option<Session>>> {
        Box::pin(async { Ok(None) })
    }
    fn session_share(&self, _session_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_unshare(&self, _session_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn session_summarize(
        &self,
        _session_id: &str,
        _provider_id: &str,
        _model_id: &str,
    ) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn experimental_session_background_list(&self) -> BoxFuture<Result<Vec<BackgroundJobInfo>>> {
        let jobs = self.background_jobs.lock().unwrap().clone();
        Box::pin(async move { Ok(jobs) })
    }
    fn experimental_session_background_status(
        &self,
        session_id: &str,
    ) -> BoxFuture<Result<BackgroundJobInfo>> {
        let job = self
            .background_jobs
            .lock()
            .unwrap()
            .iter()
            .find(|job| job.id == session_id)
            .cloned();
        Box::pin(async move { job.ok_or_else(|| anyhow!("mock: background job not found")) })
    }
    fn experimental_session_background_cancel(
        &self,
        session_id: &str,
    ) -> BoxFuture<Result<BackgroundJobInfo>> {
        let session_id = session_id.to_string();
        let result = {
            let mut jobs = self.background_jobs.lock().unwrap();
            jobs.iter_mut().find(|job| job.id == session_id).map(|job| {
                job.status = "cancelled".to_string();
                job.completed_at = Some(job.started_at);
                job.clone()
            })
        };
        if result.is_some() {
            self.cancelled_background_jobs
                .lock()
                .unwrap()
                .push(session_id);
        }
        Box::pin(async move { result.ok_or_else(|| anyhow!("mock: background job not found")) })
    }
    fn session_status(&self) -> BoxFuture<Result<HashMap<String, SessionStatus>>> {
        Box::pin(async { Ok(HashMap::new()) })
    }
    fn config_providers(&self) -> BoxFuture<Result<ConfigProviders>> {
        Box::pin(async { Ok(ConfigProviders::default()) })
    }
    fn provider_list(&self) -> BoxFuture<Result<ProviderList>> {
        Box::pin(async { Ok(ProviderList::default()) })
    }
    fn app_agents(&self) -> BoxFuture<Result<Vec<Agent>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn skill_list(&self) -> BoxFuture<Result<Vec<Skill>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn config_get(&self) -> BoxFuture<Result<Config>> {
        Box::pin(async { Ok(Config::default()) })
    }
    fn command_list(&self) -> BoxFuture<Result<Vec<Command>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn experimental_capabilities(&self) -> BoxFuture<Result<ExperimentalCapabilities>> {
        Box::pin(async {
            Ok(ExperimentalCapabilities {
                background_subagents: false,
            })
        })
    }
    fn experimental_console(&self) -> BoxFuture<Result<ConsoleState>> {
        Box::pin(async { Ok(ConsoleState::default()) })
    }
    fn permission_reply(
        &self,
        _request_id: &str,
        _reply: &str,
        _message: Option<&str>,
    ) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn question_reply(
        &self,
        _request_id: &str,
        _answers: Vec<Vec<String>>,
    ) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn question_reject(&self, _request_id: &str) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn fs_find(&self, _query: &str, _limit: &str) -> BoxFuture<Result<Vec<FileSystemEntry>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn tui_control_next(&self) -> BoxFuture<Result<TuiControlRequest>> {
        Box::pin(async { Err(anyhow!("mock: TUI control queue unavailable")) })
    }

    fn tui_control_response(&self, _body: serde_json::Value) -> BoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn global_event(type_: &str, props: serde_json::Value) -> GlobalEvent {
        GlobalEvent {
            directory: "/tmp".into(),
            project: None,
            workspace: None,
            payload: EventPayload {
                id: "evt_1".into(),
                r#type: type_.into(),
                properties: props,
            },
        }
    }

    #[test]
    fn sse_parser_accumulates_data() {
        let mut parser = SseParser::default();
        assert!(parser.push_line("event: message").is_none());
        assert!(parser.push_line("data: {\"directory\":\"/a\",\"payload\":{\"id\":\"evt_1\",\"type\":\"session.status\",\"properties\":{\"sessionID\":\"s\",\"status\":{\"type\":\"idle\"}}}}").is_none());
        let event = parser.push_line("").expect("blank line flushes");
        assert_eq!(event.payload.r#type, "session.status");
        assert_eq!(event.directory, "/a");
    }

    #[test]
    fn sse_parser_ignores_heartbeats() {
        let mut parser = SseParser::default();
        assert!(parser.push_line(": heartbeat").is_none());
        assert!(parser.push_line("").is_none());
    }

    #[test]
    fn sse_parser_filters_sync_events() {
        let mut parser = SseParser::default();
        parser.push_line("data: {\"directory\":\"/a\",\"payload\":{\"id\":\"evt_1\",\"type\":\"sync\",\"properties\":{}}}");
        assert!(parser.push_line("").is_none());
    }

    #[test]
    fn mock_client_delivers_queued_events() {
        let mock = MockSdkClient::new();
        mock.events.lock().unwrap().push(global_event(
            "session.status",
            json!({ "sessionID": "s", "status": { "type": "busy" } }),
        ));
        let mut stream = mock.subscribe_events().unwrap();
        let events: Vec<GlobalEvent> = futures::executor::block_on(async {
            let mut out = Vec::new();
            while let Some(e) = stream.next().await {
                out.push(e);
            }
            out
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload.r#type, "session.status");
    }

    #[test]
    fn parse_key_stroke_roundtrip_display() {
        let stroke = crate::keymap::KeyStroke {
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
            super_: false,
            hyper: false,
            code: crate::keymap::KeyCode::Char('p'),
        };
        assert_eq!(stroke.display(), "ctrl+p");
    }
}
