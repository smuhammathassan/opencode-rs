//! `opencode acp`.
//!
//! ACP is a stdio JSON-RPC protocol. The reference command starts an
//! opencode instance and serves ACP requests over stdin/stdout; it does not
//! expose a second TCP protocol. This module keeps that boundary deliberately
//! small: the ACP service owns protocol semantics, while this file supplies
//! the HTTP SDK adapter and the bidirectional stdio transport.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use async_trait::async_trait;
use base64::Engine as _;
use futures::{stream::BoxStream, Stream, StreamExt};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};

use crate::cli::args::{AcpArgs, Cli};
use crate::cli::context::Context;
use crate::cli::network::resolve_network_options;

use oc_acp::agent::Agent;
use oc_acp::connection::AgentSideConnection;
use oc_acp::jsonrpc::{
    RequestId, RpcErrorResponse, RpcMessage, RpcNotification, RpcRequest, RpcResponse,
};
use oc_acp::sdk::{
    AgentInfo, AssistantMessage, CommandInfo, CommandRequest, Config, ConfigProviders, Event,
    FilePart, Message, MessagePartDeltaProperties, MessagePartUpdatedProperties, ModelInfo,
    ModelLimit, OpencodeClient, Part, PermissionAskedProperties, PromptRequest, ProviderInfo,
    Session, SessionCreateRequest, SessionMessageResponse, SessionTime, SkillInfo,
    SummarizeRequest, TextPart, ToolPart, ToolState, ToolStateCompleted, ToolStateError,
    ToolStatePending, ToolStateRunning, UserMessage, UserMessageModel,
};
use oc_acp::service::{Service, ServiceInput};
use oc_acp::types::{
    AuthenticateRequest, CancelNotification, CloseSessionRequest, ForkSessionRequest,
    InitializeRequest, ListSessionsRequest, LoadSessionRequest, NewSessionRequest,
    PromptRequest as AcpPromptRequest, PromptResponse, RequestError, RequestPermissionRequest,
    RequestPermissionResponse, ResumeSessionRequest, SessionUpdate, SetSessionConfigOptionRequest,
    SetSessionModeRequest, SetSessionModelRequest, StopReason, WriteTextFileRequest,
};

type ByteStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

#[derive(Clone)]
struct HttpOpencodeClient {
    base_url: String,
    client: reqwest::Client,
}

impl HttpOpencodeClient {
    fn new(base_url: impl Into<String>) -> anyhow::Result<Self> {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(password) = std::env::var_os("OPENCODE_SERVER_PASSWORD") {
            let username = std::env::var("OPENCODE_SERVER_USERNAME")
                .unwrap_or_else(|_| "opencode".to_string());
            let token = base64::engine::general_purpose::STANDARD.encode(format!(
                "{}:{}",
                username,
                password.to_string_lossy()
            ));
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Basic {token}"))?,
            );
        }
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .default_headers(headers)
                .build()?,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        query: &[(&str, String)],
    ) -> Result<Value, Value> {
        let mut request = self.client.request(method, self.url(path));
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|error| {
            json!({
                "name": "NetworkError",
                "message": error.to_string(),
            })
        })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()))
        };
        if !status.is_success() {
            let message = value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .unwrap_or_else(|| status.canonical_reason().unwrap_or("HTTP error"));
            return Err(json!({
                "name": "HttpError",
                "message": message,
                "status": status.as_u16(),
                "data": value,
            }));
        }
        Ok(value)
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, Value> {
        self.request(Method::GET, path, None, query).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, Value> {
        self.request(Method::POST, path, Some(body), &[]).await
    }

    async fn session_messages_raw(
        &self,
        session_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<SessionMessageResponse>, Value> {
        let query = limit
            .map(|limit| vec![("limit", limit.to_string())])
            .unwrap_or_default();
        let value = self
            .get(&format!("/session/{session_id}/message"), &query)
            .await?;
        let value = unwrap_data(value);
        value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|message| session_message_from_value(message, session_id))
            .collect()
    }

    async fn wait_for_assistant(
        &self,
        session_id: &str,
        before: &HashSet<String>,
    ) -> Result<AssistantMessage, Value> {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            let messages = self.session_messages_raw(session_id, None).await?;
            for message in messages.into_iter().rev() {
                if let Message::Assistant(assistant) = message.info {
                    if !before.contains(&assistant.id) {
                        return Ok(assistant);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(json!({
                    "name": "SessionPromptTimeout",
                    "message": "Timed out waiting for the session runner",
                }));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[async_trait]
impl OpencodeClient for HttpOpencodeClient {
    fn global_event(&self) -> BoxStream<'static, Option<Event>> {
        let client = self.client.clone();
        let url = self.url("/global/event");
        let stream = futures::stream::unfold(
            EventStreamState {
                client,
                url,
                response: None,
                buffer: Vec::new(),
            },
            |mut state| async move {
                if state.response.is_none() {
                    let response = state.client.get(&state.url).send().await.ok()?;
                    if !response.status().is_success() {
                        return None;
                    }
                    state.response = Some(Box::pin(response.bytes_stream()));
                }
                loop {
                    if let Some(payload) = take_sse_payload(&mut state.buffer) {
                        return Some((event_from_value(payload), state));
                    }
                    let bytes = state.response.as_mut()?.next().await?;
                    match bytes {
                        Ok(bytes) => state.buffer.extend_from_slice(&bytes),
                        Err(_) => return None,
                    }
                }
            },
        );
        Box::pin(stream)
    }

    async fn session_create(&self, request: SessionCreateRequest) -> Result<Session, Value> {
        let body = json!({
            "directory": request.directory,
            "agent": request.agent,
            "model": {
                "providerID": request.model.provider_id,
                "modelID": request.model.id,
                "variant": request.model.variant,
            },
        });
        let value = self.post("/session", body).await?;
        session_from_value(unwrap_data(value))
    }

    async fn session_get(&self, _directory: &str, session_id: &str) -> Result<Session, Value> {
        session_from_value(unwrap_data(
            self.get(&format!("/session/{session_id}"), &[]).await?,
        ))
    }

    async fn session_messages(
        &self,
        _directory: &str,
        session_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<SessionMessageResponse>, Value> {
        self.session_messages_raw(session_id, limit).await
    }

    async fn session_message(
        &self,
        _directory: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<SessionMessageResponse, Value> {
        let value = self
            .get(&format!("/session/{session_id}/message/{message_id}"), &[])
            .await?;
        session_message_from_value(unwrap_data(value), session_id)
    }

    async fn session_list(&self, directory: Option<&str>) -> Result<Vec<Session>, Value> {
        let query = directory
            .map(|directory| vec![("directory", directory.to_string())])
            .unwrap_or_default();
        let value = unwrap_data(self.get("/session", &query).await?);
        value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(session_from_value)
            .collect()
    }

    async fn session_abort(&self, _directory: &str, session_id: &str) -> Result<(), Value> {
        self.post(&format!("/session/{session_id}/abort"), json!({}))
            .await
            .map(|_| ())
    }

    async fn session_prompt(&self, request: PromptRequest) -> Result<AssistantMessage, Value> {
        let before = self
            .session_messages_raw(&request.session_id, None)
            .await?
            .into_iter()
            .map(|message| message.info.id().to_string())
            .collect::<HashSet<_>>();
        let parts = request
            .parts
            .into_iter()
            .map(|part| serde_json::to_value(part).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        self.post(
            &format!("/session/{}/message", request.session_id),
            json!({
                "directory": request.directory,
                "agent": request.agent,
                "model": {
                    "providerID": request.model.provider_id,
                    "modelID": request.model.model_id,
                    "variant": request.variant,
                },
                "parts": parts,
            }),
        )
        .await?;
        self.wait_for_assistant(&request.session_id, &before).await
    }

    async fn session_command(&self, request: CommandRequest) -> Result<AssistantMessage, Value> {
        let before = self
            .session_messages_raw(&request.session_id, None)
            .await?
            .into_iter()
            .map(|message| message.info.id().to_string())
            .collect::<HashSet<_>>();
        let (provider_id, model_id) = request
            .model
            .split_once('/')
            .map(|(provider, model)| (provider.to_string(), model.to_string()))
            .unwrap_or_else(|| (String::new(), request.model.clone()));
        self.post(
            &format!("/session/{}/command", request.session_id),
            json!({
                "command": request.command,
                "arguments": request.arguments,
                "directory": request.directory,
                "agent": request.agent,
                "model": { "providerID": provider_id, "modelID": model_id, "variant": request.variant },
                "prompt": { "text": format!("/{} {}", request.command, request.arguments).trim() },
            }),
        )
        .await?;
        self.wait_for_assistant(&request.session_id, &before).await
    }

    async fn session_summarize(&self, request: SummarizeRequest) -> Result<bool, Value> {
        let value = self
            .post(
                &format!("/session/{}/summarize", request.session_id),
                json!({
                    "providerID": request.provider_id,
                    "modelID": request.model_id,
                }),
            )
            .await?;
        Ok(value
            .as_bool()
            .unwrap_or_else(|| value.get("data").and_then(Value::as_bool).unwrap_or(false)))
    }

    async fn session_fork(&self, _directory: &str, session_id: &str) -> Result<Session, Value> {
        let value = self
            .post(&format!("/session/{session_id}/fork"), json!({}))
            .await?;
        session_from_value(unwrap_data(value))
    }

    async fn config_providers(&self, _directory: &str) -> Result<ConfigProviders, Value> {
        let value = self.get("/config/providers", &[]).await?;
        let providers = value
            .get("providers")
            .or_else(|| value.get("data").and_then(|data| data.get("providers")))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(provider_from_value)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ConfigProviders { providers })
    }

    async fn config_get(&self, _directory: &str) -> Result<Config, Value> {
        let value = unwrap_data(self.get("/config", &[]).await?);
        Ok(Config {
            model: value.get("model").and_then(config_model_id),
        })
    }

    async fn app_agents(&self, _directory: &str) -> Result<Vec<AgentInfo>, Value> {
        let value = unwrap_data(self.get("/agent", &[]).await?);
        Ok(value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|value| AgentInfo {
                name: value
                    .get("id")
                    .or_else(|| value.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("build")
                    .to_string(),
                description: value
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                mode: value
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("all")
                    .to_string(),
                hidden: value.get("hidden").and_then(Value::as_bool),
            })
            .collect())
    }

    async fn app_skills(&self, _directory: &str) -> Result<Vec<SkillInfo>, Value> {
        let value = unwrap_data(self.get("/skill", &[]).await?);
        Ok(value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|value| SkillInfo {
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                description: value
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                content: value
                    .get("content")
                    .or_else(|| value.get("template"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
            .collect())
    }

    async fn command_list(&self, _directory: &str) -> Result<Vec<CommandInfo>, Value> {
        let value = unwrap_data(self.get("/command", &[]).await?);
        Ok(value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|value| CommandInfo {
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                description: value
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                template: value.get("template").cloned().unwrap_or(Value::Null),
                hints: Vec::new(),
                source: value
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
            .collect())
    }

    async fn mcp_add(&self, _directory: &str, name: &str, config: Value) -> Result<Value, Value> {
        self.post("/mcp", json!({ "name": name, "config": config }))
            .await
    }

    async fn permission_reply(
        &self,
        request_id: &str,
        reply: &str,
        _directory: &str,
    ) -> Result<bool, Value> {
        let value = self
            .post(
                &format!("/permission/{request_id}/reply"),
                json!({ "reply": reply }),
            )
            .await?;
        Ok(value.as_bool().unwrap_or(true))
    }
}

struct EventStreamState {
    client: reqwest::Client,
    url: String,
    response: Option<ByteStream>,
    buffer: Vec<u8>,
}

fn take_sse_payload(buffer: &mut Vec<u8>) -> Option<Value> {
    let end = buffer.windows(2).position(|window| window == b"\n\n")?;
    let frame = buffer.drain(..end + 2).collect::<Vec<_>>();
    let mut data = String::new();
    for line in String::from_utf8_lossy(&frame).lines() {
        if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.trim());
        }
    }
    (!data.is_empty())
        .then(|| serde_json::from_str(&data).ok())
        .flatten()
}

fn event_from_value(value: Value) -> Option<Event> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let properties = value
        .get("properties")
        .or_else(|| value.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    match event_type {
        "session.status" => {
            serde_json::from_value::<oc_acp::sdk::SessionStatusProperties>(properties)
                .ok()
                .map(|properties| Event::SessionStatus { id, properties })
        }
        "permission.asked" => serde_json::from_value::<PermissionAskedProperties>(properties)
            .ok()
            .map(|properties| Event::PermissionAsked { id, properties }),
        "message.part.updated" => {
            let mut properties = properties;
            normalize_event_part(&mut properties);
            serde_json::from_value::<MessagePartUpdatedProperties>(properties)
                .ok()
                .map(|properties| Event::MessagePartUpdated { id, properties })
        }
        "message.part.delta" => serde_json::from_value::<MessagePartDeltaProperties>(properties)
            .ok()
            .map(|properties| Event::MessagePartDelta { id, properties }),
        _ => Some(Event::Other(value)),
    }
}

fn normalize_event_part(properties: &mut Value) {
    let Some(object) = properties.as_object_mut() else {
        return;
    };
    let session_id = object.get("sessionID").cloned().unwrap_or(Value::Null);
    let Some(part) = object.get_mut("part").and_then(Value::as_object_mut) else {
        return;
    };
    if !part.contains_key("sessionID") {
        part.insert("sessionID".into(), session_id);
    }
    if part.get("type").and_then(Value::as_str) == Some("tool") {
        let message_id = part
            .get("messageID")
            .cloned()
            .or_else(|| part.get("assistantMessageID").cloned())
            .unwrap_or_else(|| Value::String(String::new()));
        part.insert("messageID".into(), message_id);
        if !part.contains_key("callID") {
            if let Some(id) = part.get("id").cloned() {
                part.insert("callID".into(), id);
            }
        }
        if !part.contains_key("tool") {
            if let Some(name) = part.get("name").cloned() {
                part.insert("tool".into(), name);
            }
        }
    }
}

fn unwrap_data(value: Value) -> Value {
    value.get("data").cloned().unwrap_or(value)
}

fn parse_i64(value: Option<&Value>) -> i64 {
    value
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .and_then(Value::as_str)
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_default()
}

fn config_model_id(model: &Value) -> Option<String> {
    model.as_str().map(str::to_string).or_else(|| {
        let provider = model
            .get("providerID")
            .or_else(|| model.get("providerId"))
            .and_then(Value::as_str)?;
        let id = model
            .get("id")
            .or_else(|| model.get("modelID"))
            .or_else(|| model.get("modelId"))
            .and_then(Value::as_str)?;
        Some(format!("{provider}/{id}"))
    })
}

fn session_from_value(value: Value) -> Result<Session, Value> {
    let directory = value
        .get("directory")
        .or_else(|| {
            value
                .get("location")
                .and_then(|location| location.get("directory"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(Session {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        directory,
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("New Session")
            .to_string(),
        time: SessionTime {
            created: parse_i64(value.get("time").and_then(|time| time.get("created"))),
            updated: parse_i64(value.get("time").and_then(|time| time.get("updated"))),
        },
    })
}

fn session_message_from_value(
    value: Value,
    session_id: &str,
) -> Result<SessionMessageResponse, Value> {
    let info_value = value.get("info").cloned().unwrap_or_else(|| value.clone());
    let info = message_from_value(info_value, session_id)?;
    let parts = value
        .get("parts")
        .or_else(|| value.get("content"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let parts = if parts.is_empty() {
        match &info {
            Message::User(user) => vec![Part::Text(TextPart {
                id: format!("{}_text", user.id),
                session_id: user.session_id.clone(),
                message_id: user.id.clone(),
                text: value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                synthetic: None,
                ignored: None,
                metadata: None,
            })],
            _ => Vec::new(),
        }
    } else {
        parts
            .into_iter()
            .filter_map(|part| part_from_value(part, session_id, info.id()))
            .collect()
    };
    Ok(SessionMessageResponse { info, parts })
}

fn message_from_value(value: Value, session_id: &str) -> Result<Message, Value> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let role = value
        .get("role")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("user");
    if role == "assistant" {
        let model = value.get("model").and_then(model_from_value);
        Ok(Message::Assistant(AssistantMessage {
            id,
            session_id: session_id.to_string(),
            role: "assistant".into(),
            provider_id: value
                .get("providerID")
                .or_else(|| value.get("providerId"))
                .or_else(|| value.get("model").and_then(|model| model.get("providerID")))
                .or_else(|| value.get("model").and_then(|model| model.get("providerId")))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            model_id: value
                .get("modelID")
                .or_else(|| value.get("modelId"))
                .or_else(|| value.get("model").and_then(|model| model.get("id")))
                .or_else(|| value.get("model").and_then(|model| model.get("modelID")))
                .or_else(|| value.get("model").and_then(|model| model.get("modelId")))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            mode: value
                .get("mode")
                .and_then(Value::as_str)
                .map(str::to_string),
            agent: value
                .get("agent")
                .and_then(Value::as_str)
                .map(str::to_string),
            cost: value
                .get("cost")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            tokens: tokens_from_value(value.get("tokens")),
            variant: value
                .get("variant")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| model.as_ref().and_then(|model| model.variant.clone())),
            error: value.get("error").cloned(),
            path: value.get("path").and_then(|path| {
                Some(oc_acp::sdk::MessagePath {
                    cwd: path.get("cwd")?.as_str()?.to_string(),
                    root: path.get("root")?.as_str()?.to_string(),
                })
            }),
            model,
        }))
    } else {
        Ok(Message::User(UserMessage {
            id,
            session_id: session_id.to_string(),
            role: "user".into(),
            model: value.get("model").and_then(model_from_value),
            agent: value
                .get("agent")
                .and_then(Value::as_str)
                .map(str::to_string),
        }))
    }
}

fn model_from_value(value: &Value) -> Option<UserMessageModel> {
    let object = value.as_object()?;
    let provider_id = object
        .get("providerID")
        .or_else(|| object.get("providerId"))
        .or_else(|| object.get("provider_id"))
        .and_then(Value::as_str)?;
    let model_id = object
        .get("modelID")
        .or_else(|| object.get("modelId"))
        .or_else(|| object.get("id"))
        .and_then(Value::as_str)?;
    Some(UserMessageModel {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        variant: object
            .get("variant")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn tokens_from_value(value: Option<&Value>) -> oc_acp::sdk::Tokens {
    oc_acp::sdk::Tokens {
        input: number_as_u64(value.and_then(|value| value.get("input"))),
        output: number_as_u64(value.and_then(|value| value.get("output"))),
        reasoning: number_as_u64(value.and_then(|value| value.get("reasoning"))),
        cache: oc_acp::sdk::CacheTokens {
            read: number_as_u64(
                value
                    .and_then(|value| value.get("cache"))
                    .and_then(|cache| cache.get("read")),
            ),
            write: number_as_u64(
                value
                    .and_then(|value| value.get("cache"))
                    .and_then(|cache| cache.get("write")),
            ),
        },
    }
}

fn number_as_u64(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .and_then(Value::as_f64)
                .map(|value| value.max(0.0) as u64)
        })
        .unwrap_or_default()
}

fn part_from_value(value: Value, session_id: &str, message_id: &str) -> Option<Part> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "text" => Some(Part::Text(TextPart {
            id: value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            session_id: value
                .get("sessionID")
                .and_then(Value::as_str)
                .unwrap_or(session_id)
                .to_string(),
            message_id: value
                .get("messageID")
                .and_then(Value::as_str)
                .unwrap_or(message_id)
                .to_string(),
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            synthetic: value.get("synthetic").and_then(Value::as_bool),
            ignored: value.get("ignored").and_then(Value::as_bool),
            metadata: value.get("metadata").and_then(Value::as_object).cloned(),
        })),
        "reasoning" => Some(Part::Reasoning(oc_acp::sdk::ReasoningPart {
            id: value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            session_id: value
                .get("sessionID")
                .and_then(Value::as_str)
                .unwrap_or(session_id)
                .to_string(),
            message_id: value
                .get("messageID")
                .and_then(Value::as_str)
                .unwrap_or(message_id)
                .to_string(),
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            metadata: value.get("metadata").and_then(Value::as_object).cloned(),
        })),
        "file" => Some(Part::File(FilePart {
            id: value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            session_id: value
                .get("sessionID")
                .and_then(Value::as_str)
                .unwrap_or(session_id)
                .to_string(),
            message_id: value
                .get("messageID")
                .and_then(Value::as_str)
                .unwrap_or(message_id)
                .to_string(),
            mime: value
                .get("mime")
                .or_else(|| value.get("mimeType"))
                .and_then(Value::as_str)
                .unwrap_or("text/plain")
                .to_string(),
            filename: value
                .get("filename")
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string),
            url: value
                .get("url")
                .or_else(|| value.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })),
        "tool" => tool_part_from_value(value, session_id, message_id).map(Part::Tool),
        _ => Some(Part::Other(value)),
    }
}

fn tool_part_from_value(value: Value, session_id: &str, message_id: &str) -> Option<ToolPart> {
    let state = value.get("state")?;
    let input = state
        .get("input")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let state = match state
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("running")
    {
        "pending" => ToolState::Pending(ToolStatePending {
            input,
            raw: state
                .get("raw")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        "completed" => ToolState::Completed(ToolStateCompleted {
            input,
            output: tool_output(state),
            title: state
                .get("title")
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            metadata: state
                .get("metadata")
                .or_else(|| state.get("structured"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            attachments: state
                .get("attachments")
                .or_else(|| value.get("attachments"))
                .and_then(Value::as_array)
                .map(|attachments| {
                    attachments
                        .iter()
                        .filter_map(|attachment| {
                            file_part_from_value(attachment, session_id, message_id)
                        })
                        .collect()
                }),
        }),
        "error" => ToolState::Error(ToolStateError {
            input,
            error: state
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| state.get("error").map(Value::to_string).unwrap_or_default()),
            metadata: state
                .get("metadata")
                .or_else(|| state.get("structured"))
                .and_then(Value::as_object)
                .cloned(),
        }),
        _ => ToolState::Running(ToolStateRunning {
            input,
            title: state
                .get("title")
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string),
            metadata: state
                .get("metadata")
                .or_else(|| state.get("structured"))
                .and_then(Value::as_object)
                .cloned(),
        }),
    };
    Some(ToolPart {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        session_id: session_id.to_string(),
        message_id: value
            .get("messageID")
            .and_then(Value::as_str)
            .unwrap_or(message_id)
            .to_string(),
        call_id: value
            .get("callID")
            .or_else(|| value.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tool: value
            .get("tool")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string(),
        state,
        metadata: value.get("metadata").and_then(Value::as_object).cloned(),
    })
}

fn file_part_from_value(value: &Value, session_id: &str, message_id: &str) -> Option<FilePart> {
    Some(FilePart {
        id: value.get("id").and_then(Value::as_str)?.to_string(),
        session_id: value
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or(session_id)
            .to_string(),
        message_id: value
            .get("messageID")
            .and_then(Value::as_str)
            .unwrap_or(message_id)
            .to_string(),
        mime: value
            .get("mime")
            .or_else(|| value.get("mimeType"))
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_string(),
        filename: value
            .get("filename")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        url: value
            .get("url")
            .or_else(|| value.get("uri"))
            .and_then(Value::as_str)?
            .to_string(),
    })
}

fn tool_output(state: &Value) -> String {
    let value = state.get("output").or_else(|| state.get("content"));
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .or_else(|| item.get("text").and_then(Value::as_str).map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join(""),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn provider_from_value(value: Value) -> Result<ProviderInfo, Value> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let models = value
        .get("models")
        .and_then(Value::as_object)
        .map(|models| {
            models
                .iter()
                .map(|(key, model)| {
                    let model_id = model
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or(key)
                        .to_string();
                    let limit = model.get("limit").map(|limit| ModelLimit {
                        context: limit
                            .get("context")
                            .and_then(Value::as_f64)
                            .unwrap_or_default(),
                        output: limit
                            .get("output")
                            .and_then(Value::as_f64)
                            .unwrap_or_default(),
                    });
                    (
                        key.clone(),
                        ModelInfo {
                            id: model_id,
                            provider_id: model
                                .get("providerID")
                                .or_else(|| model.get("providerId"))
                                .and_then(Value::as_str)
                                .unwrap_or(&id)
                                .to_string(),
                            name: model
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or(key)
                                .to_string(),
                            variants: model.get("variants").and_then(Value::as_object).map(
                                |variants| {
                                    variants
                                        .iter()
                                        .filter_map(|(key, value)| {
                                            value
                                                .as_object()
                                                .cloned()
                                                .map(|value| (key.clone(), value))
                                        })
                                        .collect()
                                },
                            ),
                            limit,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ProviderInfo {
        id,
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        source: value
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("env")
            .to_string(),
        env: value
            .get("env")
            .and_then(Value::as_array)
            .map(|env| {
                env.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        key: value.get("key").and_then(Value::as_str).map(str::to_string),
        options: value
            .get("options")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        models,
    })
}

#[derive(Clone)]
struct StdioConnection {
    output: Arc<Mutex<tokio::io::Stdout>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, RequestError>>>>>,
    sequence: Arc<AtomicU64>,
}

#[derive(Clone, Default)]
struct PromptCancellationRegistry {
    sequence: Arc<AtomicU64>,
    waiters: Arc<Mutex<HashMap<String, HashMap<u64, oneshot::Sender<()>>>>>,
}

struct PromptRegistration {
    session_id: String,
    id: u64,
    receiver: oneshot::Receiver<()>,
}

impl PromptCancellationRegistry {
    async fn register(&self, session_id: &str) -> PromptRegistration {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.waiters
            .lock()
            .await
            .entry(session_id.to_string())
            .or_default()
            .insert(id, sender);
        PromptRegistration {
            session_id: session_id.to_string(),
            id,
            receiver,
        }
    }

    async fn cancel(&self, session_id: &str) {
        let waiters = self.waiters.lock().await.remove(session_id);
        if let Some(waiters) = waiters {
            for sender in waiters.into_values() {
                let _ = sender.send(());
            }
        }
    }

    async fn unregister(&self, session_id: &str, id: u64) {
        let mut waiters = self.waiters.lock().await;
        if let Some(session_waiters) = waiters.get_mut(session_id) {
            session_waiters.remove(&id);
            if session_waiters.is_empty() {
                waiters.remove(session_id);
            }
        }
    }
}

impl StdioConnection {
    fn new() -> Self {
        Self {
            output: Arc::new(Mutex::new(tokio::io::stdout())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            sequence: Arc::new(AtomicU64::new(1)),
        }
    }

    async fn send(&self, message: RpcMessage) -> io::Result<()> {
        let data = serde_json::to_vec(&message)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut output = self.output.lock().await;
        output.write_all(&data).await?;
        output.write_all(b"\n").await?;
        output.flush().await
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, ()> {
        let id = RequestId::Number(self.sequence.fetch_add(1, Ordering::Relaxed) as i64);
        let key = request_id_key(&id);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(key.clone(), sender);
        if self
            .send(RpcMessage::Request(RpcRequest {
                jsonrpc: "2.0".into(),
                id,
                method: method.into(),
                params: Some(params),
            }))
            .await
            .is_err()
        {
            self.pending.lock().await.remove(&key);
            return Err(());
        }
        receiver.await.map_err(|_| ())?.map_err(|_| ())
    }

    async fn resolve(&self, id: &RequestId, result: Result<Value, RequestError>) {
        if let Some(sender) = self.pending.lock().await.remove(&request_id_key(id)) {
            let _ = sender.send(result);
        }
    }
}

#[async_trait]
impl AgentSideConnection for StdioConnection {
    async fn session_update(&self, session_id: &str, update: SessionUpdate) -> Result<(), ()> {
        let mut params = serde_json::to_value(update)
            .map_err(|_| ())?
            .as_object()
            .cloned()
            .ok_or(())?;
        params.insert("sessionId".into(), Value::String(session_id.to_string()));
        self.send(RpcMessage::Notification(RpcNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: Some(Value::Object(params)),
        }))
        .await
        .map_err(|_| ())
    }

    async fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, ()> {
        let value = self
            .request(
                "session/request_permission",
                serde_json::to_value(request).map_err(|_| ())?,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| ())
    }

    async fn write_text_file(&self, request: WriteTextFileRequest) -> Result<(), ()> {
        self.request(
            "fs/write_text_file",
            serde_json::to_value(request).map_err(|_| ())?,
        )
        .await
        .map(|_| ())
    }
}

fn request_id_key(id: &RequestId) -> String {
    serde_json::to_string(id).unwrap_or_else(|_| "null".into())
}

fn decode_params<T: DeserializeOwned>(
    params: Option<Value>,
    method: &str,
) -> Result<T, RequestError> {
    serde_json::from_value(params.unwrap_or_else(|| json!({}))).map_err(|error| {
        RequestError::invalid_params(
            Some(json!({ "method": method, "error": error.to_string() })),
            None,
        )
    })
}

async fn dispatch(
    agent: Arc<Agent>,
    method: String,
    params: Option<Value>,
    cancellations: PromptCancellationRegistry,
    prompt_registration: Option<PromptRegistration>,
) -> Result<Value, RequestError> {
    macro_rules! call {
        ($ty:ty, $function:ident) => {{
            let params: $ty = decode_params(params, &method)?;
            serde_json::to_value(agent.$function(&params).await?).map_err(|error| {
                RequestError::internal_error(Some(json!({ "error": error.to_string() })), None)
            })
        }};
    }
    match method.as_str() {
        "initialize" => call!(InitializeRequest, initialize),
        "authenticate" => call!(AuthenticateRequest, authenticate),
        "session/new" => call!(NewSessionRequest, new_session),
        "session/load" => call!(LoadSessionRequest, load_session),
        "session/list" => call!(ListSessionsRequest, list_sessions),
        "session/resume" => call!(ResumeSessionRequest, resume_session),
        "session/close" => call!(CloseSessionRequest, close_session),
        "session/fork" | "unstable_forkSession" => call!(ForkSessionRequest, fork_session),
        "session/set_config_option" => {
            call!(SetSessionConfigOptionRequest, set_session_config_option)
        }
        "session/set_mode" => call!(SetSessionModeRequest, set_session_mode),
        "session/set_model" | "unstable_setSessionModel" => {
            call!(SetSessionModelRequest, set_session_model)
        }
        "session/prompt" => {
            let params: AcpPromptRequest = match decode_params(params, &method) {
                Ok(params) => params,
                Err(error) => {
                    if let Some(registration) = prompt_registration {
                        cancellations
                            .unregister(&registration.session_id, registration.id)
                            .await;
                    }
                    return Err(error);
                }
            };
            let registration = match prompt_registration {
                Some(registration) => registration,
                None => cancellations.register(&params.session_id).await,
            };
            let response = await_prompt_with_cancellation(
                &params,
                &cancellations,
                registration,
                agent.prompt(&params),
            )
            .await?;
            serde_json::to_value(response).map_err(|error| {
                RequestError::internal_error(Some(json!({ "error": error.to_string() })), None)
            })
        }
        "session/cancel" => {
            let params: CancelNotification = decode_params(params, &method)?;
            cancel_session(&agent, &cancellations, &params).await?;
            Ok(Value::Null)
        }
        _ => Err(RequestError::method_not_found(&method)),
    }
}

async fn cancel_session(
    agent: &Agent,
    cancellations: &PromptCancellationRegistry,
    params: &CancelNotification,
) -> Result<(), RequestError> {
    agent.cancel(params).await?;
    cancellations.cancel(&params.session_id).await;
    Ok(())
}

async fn await_prompt_with_cancellation<F>(
    params: &AcpPromptRequest,
    cancellations: &PromptCancellationRegistry,
    registration: PromptRegistration,
    prompt: F,
) -> Result<PromptResponse, RequestError>
where
    F: Future<Output = Result<PromptResponse, RequestError>> + Send,
{
    let PromptRegistration {
        session_id,
        id,
        receiver,
    } = registration;
    let result = tokio::select! {
        biased;
        _ = receiver => Ok(PromptResponse {
            stop_reason: StopReason::Cancelled,
            usage: None,
            user_message_id: params.message_id.clone(),
            _meta: serde_json::Map::new(),
        }),
        result = prompt => result,
    };
    cancellations.unregister(&session_id, id).await;
    result
}

async fn handle_message(
    message: RpcMessage,
    agent: Arc<Agent>,
    connection: StdioConnection,
    cancellations: PromptCancellationRegistry,
    prompt_registration: Option<PromptRegistration>,
) {
    match message {
        RpcMessage::Request(request) => {
            let id = request.id.clone();
            let result = dispatch(
                agent,
                request.method,
                request.params,
                cancellations,
                prompt_registration,
            )
            .await;
            let response = match result {
                Ok(result) => RpcMessage::Response(RpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result,
                }),
                Err(error) => RpcMessage::Error(RpcErrorResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    error,
                }),
            };
            let _ = connection.send(response).await;
        }
        RpcMessage::Notification(notification) if notification.method == "session/cancel" => {
            if let Ok(params) =
                decode_params::<CancelNotification>(notification.params, "session/cancel")
            {
                let _ = cancel_session(&agent, &cancellations, &params).await;
            }
        }
        RpcMessage::Response(response) => {
            connection.resolve(&response.id, Ok(response.result)).await
        }
        RpcMessage::Error(response) => connection.resolve(&response.id, Err(response.error)).await,
        RpcMessage::Notification(_) => {}
    }
}

pub async fn run(_cli: &Cli, args: &AcpArgs) -> anyhow::Result<i32> {
    let directory = args
        .cwd
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    std::env::set_current_dir(&directory).with_context(|| {
        format!(
            "failed to use ACP working directory {}",
            directory.display()
        )
    })?;
    let _ctx = Context::load(directory.clone())?;
    let opts = resolve_network_options(&args.network, None);
    let mut server_opts = oc_server::server::ListenOptions::new(&opts.hostname, opts.port);
    server_opts.auth = oc_server::auth::AuthConfig::from_env();
    server_opts.cors = oc_server::cors::CorsOptions {
        cors: (!opts.cors.is_empty()).then(|| opts.cors.clone()),
    };
    server_opts.mdns = opts.mdns;
    server_opts.mdns_domain = Some(opts.mdns_domain);
    let listener = oc_server::server::listen(server_opts).await?;

    let sdk = Arc::new(HttpOpencodeClient::new(listener.url.to_string())?);
    let connection = StdioConnection::new();
    let service = Arc::new(Service::make(
        ServiceInput::new(sdk).connection(Arc::new(connection.clone())),
    ));
    let agent = Arc::new(Agent::new(service));
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut request_tasks = Vec::new();
    let cancellations = PromptCancellationRegistry::default();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message: RpcMessage = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                let _ = connection
                    .send(RpcMessage::Error(RpcErrorResponse {
                        jsonrpc: "2.0".into(),
                        id: RequestId::Null,
                        error: RequestError::parse_error(
                            Some(json!({ "error": error.to_string() })),
                            None,
                        ),
                    }))
                    .await;
                continue;
            }
        };
        if let RpcMessage::Response(response) = &message {
            connection
                .resolve(&response.id, Ok(response.result.clone()))
                .await;
            continue;
        }
        if let RpcMessage::Error(response) = &message {
            connection
                .resolve(&response.id, Err(response.error.clone()))
                .await;
            continue;
        }
        let prompt_registration = match &message {
            RpcMessage::Request(request) if request.method == "session/prompt" => {
                let session_id = request
                    .params
                    .as_ref()
                    .and_then(|params| params.get("sessionId"))
                    .and_then(Value::as_str);
                match session_id {
                    Some(session_id) => Some(cancellations.register(session_id).await),
                    None => None,
                }
            }
            _ => None,
        };
        let agent = agent.clone();
        let connection = connection.clone();
        let cancellations = cancellations.clone();
        request_tasks.push(tokio::spawn(async move {
            handle_message(
                message,
                agent,
                connection,
                cancellations,
                prompt_registration,
            )
            .await
        }));
    }
    for task in request_tasks {
        let _ = task.await;
    }
    listener.stop(false).await;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_data_frame() {
        let mut buffer = b"event: message\ndata: {\"type\":\"server.connected\"}\n\n".to_vec();
        assert_eq!(
            take_sse_payload(&mut buffer),
            Some(json!({ "type": "server.connected" }))
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn maps_flat_assistant_message_to_acp_parts() {
        let message = session_message_from_value(
            json!({
                "id": "msg_1",
                "type": "assistant",
                "providerID": "openai",
                "modelID": "gpt-test",
                "text": "hello",
                "tokens": { "input": 1.5, "output": 2.0, "reasoning": 0.0, "cache": { "read": 3.0, "write": 4.0 } },
                "content": [{ "type": "text", "id": "part_1", "text": "hello" }]
            }),
            "ses_1",
        )
        .unwrap();
        let Message::Assistant(info) = message.info else {
            panic!("expected assistant message");
        };
        assert_eq!(info.provider_id, "openai");
        assert_eq!(info.tokens.input, 1);
        assert!(matches!(message.parts.first(), Some(Part::Text(_))));
    }

    #[test]
    fn maps_provider_models_with_provider_id_aliases() {
        let provider = provider_from_value(json!({
            "id": "openai",
            "name": "OpenAI",
            "source": "env",
            "models": {
                "gpt-test": {
                    "id": "gpt-test",
                    "providerID": "openai",
                    "name": "GPT Test",
                    "limit": { "context": 1000, "output": 100 }
                }
            }
        }))
        .unwrap();
        assert_eq!(provider.models["gpt-test"].provider_id, "openai");
        assert_eq!(provider.models["gpt-test"].limit.unwrap().context, 1000.0);
    }

    #[test]
    fn parses_session_status_idle_event() {
        let event = event_from_value(json!({
            "type": "session.status",
            "id": "event-1",
            "properties": {
                "sessionID": "ses_1",
                "status": { "type": "idle" }
            }
        }));
        assert!(matches!(
            event,
            Some(Event::SessionStatus { properties, .. })
                if properties.session_id == "ses_1" && properties.status.kind == "idle"
        ));
    }

    #[test]
    fn preserves_provider_file_and_tool_transcript_fields() {
        let message = session_message_from_value(
            json!({
                "id": "msg_2",
                "type": "assistant",
                "providerID": "anthropic",
                "modelID": "claude-sonnet-4",
                "model": { "providerID": "anthropic", "id": "claude-sonnet-4", "variant": "high" },
                "parts": [
                    {
                        "type": "file",
                        "id": "file_1",
                        "sessionID": "ses_1",
                        "messageID": "msg_2",
                        "mime": "text/plain",
                        "filename": "notes.txt",
                        "url": "file:///tmp/notes.txt"
                    },
                    {
                        "type": "tool",
                        "id": "part_1",
                        "callID": "call_1",
                        "tool": "read",
                        "state": {
                            "status": "completed",
                            "input": { "filePath": "/tmp/notes.txt" },
                            "content": "read ok",
                            "title": "Read notes",
                            "metadata": { "source": "fixture" },
                            "attachments": [{
                                "id": "attachment_1",
                                "mime": "image/png",
                                "url": "data:image/png;base64,AQI="
                            }]
                        }
                    }
                ]
            }),
            "ses_1",
        )
        .unwrap();

        let Message::Assistant(info) = message.info else {
            panic!("expected assistant message");
        };
        assert_eq!(info.provider_id, "anthropic");
        assert_eq!(info.model_id, "claude-sonnet-4");
        assert_eq!(info.variant.as_deref(), Some("high"));
        let Part::File(file) = &message.parts[0] else {
            panic!("expected file part");
        };
        assert_eq!(file.session_id, "ses_1");
        assert_eq!(file.message_id, "msg_2");
        let Part::Tool(tool) = &message.parts[1] else {
            panic!("expected tool part");
        };
        let ToolState::Completed(state) = &tool.state else {
            panic!("expected completed tool state");
        };
        assert_eq!(state.output, "read ok");
        assert_eq!(state.attachments.as_ref().unwrap().len(), 1);
        assert_eq!(state.metadata["source"], "fixture");
    }

    #[tokio::test]
    async fn cancels_in_flight_prompt_with_acp_stop_reason() {
        let cancellations = PromptCancellationRegistry::default();
        let registration = cancellations.register("ses_1").await;
        let params = AcpPromptRequest {
            session_id: "ses_1".into(),
            prompt: Vec::new(),
            message_id: Some("msg_1".into()),
            _meta: None,
        };
        let cancel_task = {
            let cancellations = cancellations.clone();
            tokio::spawn(async move {
                tokio::task::yield_now().await;
                cancellations.cancel("ses_1").await;
            })
        };

        let response = await_prompt_with_cancellation(
            &params,
            &cancellations,
            registration,
            futures::future::pending::<Result<PromptResponse, RequestError>>(),
        )
        .await
        .unwrap();

        cancel_task.await.unwrap();
        assert_eq!(response.stop_reason, StopReason::Cancelled);
        assert_eq!(response.user_message_id.as_deref(), Some("msg_1"));
        assert!(cancellations.waiters.lock().await.is_empty());
    }
}
