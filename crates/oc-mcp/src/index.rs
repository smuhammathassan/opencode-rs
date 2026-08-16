//! The MCP service: manages connections to configured MCP servers, exposes
//! their tools/prompts/resources, and drives the OAuth flows.
//!
//! From reference/packages/opencode/src/mcp/index.ts. The reference uses the
//! Effect framework; this port uses async/await with an `Arc<RwLock>` state.

use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use url::Url;

use crate::auth::McpAuth;
use crate::browser::McpBrowser;
use crate::catalog;
use crate::client::{register_roots_handler, Client};
use crate::config::{Info, Local, Remote};
use crate::crypto::random_hex;
use crate::oauth_callback;
use crate::oauth_provider::{
    McpOAuthCallbacks, McpOAuthConfig, McpOAuthPendingProvider, McpOAuthProvider,
    OAuthClientProvider, OAUTH_CALLBACK_PATH,
};
use crate::transport::http::StreamableHTTPClientTransport;
use crate::transport::sse::SSEClientTransport;
use crate::transport::stdio::StdioTransport;
use crate::transport::Transport;
use crate::types::{
    ClientCapabilities, GetPromptResult, Implementation, LoggingMessageNotificationParams,
    ReadResourceResult, Tool,
};
use crate::util::now_seconds;
use crate::Result;

pub const DEFAULT_TIMEOUT: u64 = 30_000;
pub const DEFAULT_REQUEST_TIMEOUT: u64 = 60_000;

/// Event emitted by the MCP service.
/// From reference/packages/schema/src/mcp-event.ts.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum McpEvent {
    #[serde(rename = "mcp.tools.changed")]
    ToolsChanged { server: String },
    #[serde(rename = "mcp.browser.open.failed")]
    BrowserOpenFailed {
        #[serde(rename = "mcpName")]
        mcp_name: String,
        url: String,
    },
}

/// From reference `MCP.ServerInstructions`.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInstructions {
    pub name: String,
    pub instructions: String,
    pub tools: Vec<String>,
}

/// An MCP tool in its native shape; consumers adapt it to their own tool format.
#[derive(Clone)]
pub struct McpTool {
    pub def: Tool,
    pub client: Arc<Client>,
    pub timeout: Option<u64>,
}

/// From reference `MCP.Status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    Connected,
    Disabled,
    Failed { error: String },
    NeedsAuth,
    NeedsClientRegistration { error: String },
}

impl Status {
    pub fn is_connected(&self) -> bool {
        matches!(self, Status::Connected)
    }
}

/// From reference `MCP.AuthStatus`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    Authenticated,
    Expired,
    NotAuthenticated,
}

pub struct McpOptions {
    pub auth: Option<Arc<McpAuth>>,
    pub default_timeout: Option<u64>,
    pub events: Option<mpsc::UnboundedSender<McpEvent>>,
}

impl Default for McpOptions {
    fn default() -> Self {
        McpOptions {
            auth: None,
            default_timeout: None,
            events: None,
        }
    }
}

struct CreateResult {
    client: Option<Arc<Client>>,
    status: Status,
    defs: Option<Vec<Tool>>,
    instructions: Option<String>,
}

#[derive(Clone)]
struct PendingOAuth {
    transport: Arc<dyn Transport>,
    provider: Option<Arc<McpOAuthPendingProvider>>,
}

struct State {
    config: IndexMap<String, Info>,
    status: IndexMap<String, Status>,
    clients: IndexMap<String, Arc<Client>>,
    defs: IndexMap<String, Vec<Tool>>,
    instructions: IndexMap<String, String>,
}

pub struct Mcp {
    state: Arc<RwLock<State>>,
    auth: Arc<McpAuth>,
    directory: PathBuf,
    default_timeout: Option<u64>,
    events: Option<mpsc::UnboundedSender<McpEvent>>,
    client_info: Implementation,
    pending_oauth: Mutex<IndexMap<String, PendingOAuth>>,
}

fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        // https://github.com/anomalyco/opencode/issues/11948, 23066, 2308, 28567
        roots: Some(serde_json::json!({})),
        sampling: None,
        experimental: None,
    }
}

impl Mcp {
    pub fn new(config: IndexMap<String, Info>, directory: PathBuf) -> Arc<Self> {
        Mcp::with_options(config, directory, McpOptions::default())
    }

    pub fn with_options(
        config: IndexMap<String, Info>,
        directory: PathBuf,
        options: McpOptions,
    ) -> Arc<Self> {
        let auth = options.auth.unwrap_or_else(|| Arc::new(McpAuth::default()));
        Arc::new(Mcp {
            state: Arc::new(RwLock::new(State {
                config,
                status: IndexMap::new(),
                clients: IndexMap::new(),
                defs: IndexMap::new(),
                instructions: IndexMap::new(),
            })),
            auth,
            directory,
            default_timeout: options.default_timeout,
            events: options.events,
            client_info: Implementation {
                name: "opencode".into(),
                version: crate::version().into(),
            },
            pending_oauth: Mutex::new(IndexMap::new()),
        })
    }

    /// Eagerly connect all enabled servers, mirroring the reference's state
    /// initialization (unbounded concurrency).
    pub async fn init(self: &Arc<Self>) {
        let configs: Vec<(String, Info)> = self
            .state
            .read()
            .await
            .config
            .iter()
            .map(|(key, info)| (key.clone(), info.clone()))
            .collect();

        let mut handles = Vec::new();
        for (key, mcp) in configs {
            if !mcp.enabled() {
                self.state
                    .write()
                    .await
                    .status
                    .insert(key.clone(), Status::Disabled);
                continue;
            }
            let this = Arc::clone(self);
            handles.push(tokio::spawn(async move {
                let result = this.create(&key, &mcp).await;
                (key, mcp, result)
            }));
        }

        for handle in handles {
            if let Ok((key, mcp, result)) = handle.await {
                let mut state = self.state.write().await;
                state.status.insert(key.clone(), result.status.clone());
                if let Some(client) = result.client {
                    state.clients.insert(key.clone(), client.clone());
                    if let Some(defs) = result.defs {
                        state.defs.insert(key.clone(), defs);
                    }
                    if let Some(instructions) = result.instructions {
                        state.instructions.insert(key.clone(), instructions);
                    }
                    drop(state);
                    self.watch(&key, &client, mcp.timeout()).await;
                }
            }
        }
    }

    /// From reference `MCP.status`.
    pub async fn status(&self) -> IndexMap<String, Status> {
        let state = self.state.read().await;
        let mut result = IndexMap::new();
        for (key, _) in &state.config {
            result.insert(
                key.clone(),
                state.status.get(key).cloned().unwrap_or(Status::Disabled),
            );
        }
        result
    }

    /// From reference `MCP.clients`.
    pub async fn clients(&self) -> IndexMap<String, Arc<Client>> {
        self.state.read().await.clients.clone()
    }

    /// From reference `MCP.instructions`.
    pub async fn instructions(&self) -> Vec<ServerInstructions> {
        let state = self.state.read().await;
        let mut names: Vec<String> = state.instructions.keys().cloned().collect();
        names.sort();
        let mut result = Vec::with_capacity(names.len());
        for name in names {
            if state
                .status
                .get(&name)
                .map(|status| status.is_connected())
                .unwrap_or(false)
            {
                result.push(ServerInstructions {
                    name: name.clone(),
                    instructions: state.instructions.get(&name).cloned().unwrap_or_default(),
                    tools: state
                        .defs
                        .get(&name)
                        .map(|defs| {
                            defs.iter()
                                .map(|tool| catalog::tool_name(&name, &tool.name))
                                .collect()
                        })
                        .unwrap_or_default(),
                });
            }
        }
        result
    }

    /// From reference `MCP.tools`.
    pub async fn tools(&self) -> IndexMap<String, McpTool> {
        let state = self.state.read().await;
        let mut result = IndexMap::new();
        for (client_name, client) in &state.clients {
            if !state
                .status
                .get(client_name)
                .map(|status| status.is_connected())
                .unwrap_or(false)
            {
                continue;
            }
            let mcp_config = state.config.get(client_name);
            let Some(listed) = state.defs.get(client_name) else {
                warn!(
                    message = "missing cached tools for connected server",
                    client_name = tracing::field::debug(client_name)
                );
                continue;
            };
            let timeout = request_timeout(mcp_config, self.default_timeout);
            for def in listed {
                result.insert(
                    catalog::tool_name(client_name, &def.name),
                    McpTool {
                        def: def.clone(),
                        client: client.clone(),
                        timeout,
                    },
                );
            }
        }
        result
    }

    /// From reference `MCP.prompts`.
    pub async fn prompts(&self) -> Result<IndexMap<String, serde_json::Value>> {
        self.collect_from_connected(
            "prompts",
            None::<&catalog::KeyFn<crate::types::Prompt>>,
            |client, timeout| catalog::prompts(client, timeout),
            None,
        )
        .await
    }

    /// From reference `MCP.resources`.
    pub async fn resources(
        &self,
        client_name: Option<&str>,
    ) -> Result<IndexMap<String, serde_json::Value>> {
        self.collect_from_connected(
            "resources",
            Some(&|resource: &crate::types::Resource| resource.uri.clone()),
            |client, timeout| catalog::resources(client, timeout),
            client_name,
        )
        .await
    }

    /// From reference `MCP.resourceTemplates`.
    pub async fn resource_templates(
        &self,
        client_name: Option<&str>,
    ) -> Result<IndexMap<String, serde_json::Value>> {
        self.collect_from_connected(
            "resource templates",
            Some(&|template: &crate::types::ResourceTemplate| template.uri_template.clone()),
            |client, timeout| catalog::resource_templates(client, timeout),
            client_name,
        )
        .await
    }

    /// From reference `MCP.add`.
    pub async fn add(&self, name: &str, mcp: Info) -> IndexMap<String, Status> {
        self.state
            .write()
            .await
            .config
            .insert(name.to_string(), mcp.clone());
        let _ = self.create_and_store(name, &mcp).await;
        self.status().await
    }

    /// From reference `MCP.connect`.
    pub async fn connect(&self, name: &str) -> Result<()> {
        let mcp = self.require_mcp_config(name).await?;
        let mut enabled = mcp.clone();
        enabled.set_enabled(true);
        self.create_and_store(name, &enabled).await?;
        Ok(())
    }

    /// From reference `MCP.disconnect`.
    pub async fn disconnect(&self, name: &str) -> Result<()> {
        self.require_mcp_config(name).await?;
        let mut state = self.state.write().await;
        if let Some(client) = state.clients.shift_remove(name) {
            let _ = client.close().await;
        }
        state.defs.shift_remove(name);
        state.instructions.shift_remove(name);
        state.status.insert(name.to_string(), Status::Disabled);
        Ok(())
    }

    /// From reference `MCP.getPrompt`.
    pub async fn get_prompt(
        &self,
        client_name: &str,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<Option<GetPromptResult>> {
        let name = name.to_string();
        self.with_client(client_name, "getPrompt", move |client, timeout| async move {
            client.get_prompt(&name, arguments, timeout).await
        })
        .await
    }

    /// From reference `MCP.readResource`.
    pub async fn read_resource(
        &self,
        client_name: &str,
        resource_uri: &str,
    ) -> Result<Option<ReadResourceResult>> {
        let resource_uri = resource_uri.to_string();
        self.with_client(client_name, "readResource", move |client, timeout| async move {
            client.read_resource(&resource_uri, timeout).await
        })
        .await
    }

    /// From reference `MCP.startAuth`. Returns the authorization URL and state
    /// for the interactive flow, or a connected client when auth completed
    /// inline.
    pub async fn start_auth(&self, mcp_name: &str) -> Result<AuthStartResult> {
        let mcp_config = self.require_mcp_config(mcp_name).await?;
        let Info::Remote(remote) = &mcp_config else {
            return Err(crate::Error::message(format!(
                "MCP server {mcp_name} is not a remote server"
            )));
        };
        if !remote.oauth_enabled() {
            return Err(crate::Error::message(format!(
                "MCP server {mcp_name} has OAuth explicitly disabled"
            )));
        }
        let url = Url::parse(&remote.url)
            .map_err(|_| crate::Error::message(format!("Invalid MCP URL for \"{mcp_name}\"")))?;

        let oauth_config = remote.oauth_config();
        let effective_redirect_uri = oauth_config
            .and_then(|config| config.redirect_uri.clone())
            .or_else(|| {
                oauth_config
                    .and_then(|config| config.callback_port)
                    .map(|port| format!("http://127.0.0.1:{port}{OAUTH_CALLBACK_PATH}"))
            });
        let oauth_state = random_hex(32);
        self.auth
            .update_oauth_state_for_url(mcp_name, oauth_state.clone(), Some(&remote.url))
            .await?;

        let captured_url = Arc::new(Mutex::new(None::<Url>));
        let captured = Arc::clone(&captured_url);
        let callbacks = McpOAuthCallbacks {
            on_redirect: Arc::new(move |url: &Url| {
                let captured = Arc::clone(&captured);
                Box::pin(async move {
                    *captured.lock().await = Some(url.clone());
                    Ok(())
                })
            }),
        };
        let provider = Arc::new(McpOAuthPendingProvider::new(
            mcp_name,
            &remote.url,
            McpOAuthConfig::from_config(oauth_config),
            callbacks,
            Arc::clone(&self.auth),
        ));
        let transport = StreamableHTTPClientTransport::new(
            url.clone(),
            remote.headers.clone(),
            Some(provider.clone()),
        );
        let transport: Arc<dyn Transport> = Arc::new(transport);

        match self
            .connect_transport(Arc::clone(&transport), DEFAULT_TIMEOUT)
            .await
        {
            Ok(client) => {
                let _ = provider.commit().await;
                Ok(AuthStartResult {
                    authorization_url: String::new(),
                    oauth_state,
                    client: Some(client),
                })
            }
            Err(error) if error.is_unauthorized() => {
                let captured = captured_url.lock().await.clone();
                if let Some(authorization_url) = captured {
                    if let Err(callback_error) =
                        oauth_callback::ensure_running(effective_redirect_uri.as_deref()).await
                    {
                        let _ = self.auth.clear_oauth_state(mcp_name).await;
                        let _ = self.auth.clear_code_verifier(mcp_name).await;
                        return Err(callback_error);
                    }
                    self.pending_oauth.lock().await.insert(
                        mcp_name.to_string(),
                        PendingOAuth {
                            transport,
                            provider: Some(provider),
                        },
                    );
                    return Ok(AuthStartResult {
                        authorization_url: authorization_url.to_string(),
                        oauth_state,
                        client: None,
                    });
                }
                let _ = self.auth.clear_oauth_state(mcp_name).await;
                let _ = self.auth.clear_code_verifier(mcp_name).await;
                Err(error)
            }
            Err(error) => {
                let _ = self.auth.clear_oauth_state(mcp_name).await;
                let _ = self.auth.clear_code_verifier(mcp_name).await;
                Err(error)
            }
        }
    }

    /// From reference `MCP.authenticate`: opens the browser and waits for the
    /// authorization callback.
    pub async fn authenticate(
        &self,
        mcp_name: &str,
        on_authorization: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<Status> {
        let result = self.start_auth(mcp_name).await?;

        if result.authorization_url.is_empty() {
            let mcp_config = self.require_mcp_config(mcp_name).await?;
            let client = result
                .client
                .ok_or_else(|| crate::Error::message("no client from start_auth"))?;
            let listed = if client
                .get_server_capabilities()
                .await
                .map(|capabilities| capabilities.has_tools())
                .unwrap_or(false)
            {
                catalog::defs(client.clone(), mcp_config.timeout()).await?
            } else {
                Some(Vec::new())
            };
            let Some(listed) = listed else {
                let _ = client.close().await;
                return Ok(Status::Failed {
                    error: "Failed to get tools".into(),
                });
            };
            let _ = self.auth.clear_oauth_state(mcp_name).await;
            let instructions = client
                .get_instructions()
                .await
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty());
            return self
                .store_client(mcp_name, client, listed, instructions, mcp_config.timeout())
                .await;
        }

        let callback_rx =
            oauth_callback::wait_for_callback(&result.oauth_state, Some(mcp_name)).await;
        if let Some(on_authorization) = on_authorization {
            on_authorization(&result.authorization_url);
        }

        if let Err(error) = McpBrowser::open(&result.authorization_url).await {
            if let Some(events) = &self.events {
                let _ = events.send(McpEvent::BrowserOpenFailed {
                    mcp_name: mcp_name.to_string(),
                    url: result.authorization_url.clone(),
                });
            }
            warn!(message = format!("failed to open browser: {error}"));
        }

        let code = match callback_rx.await {
            Ok(Ok(code)) => code,
            Ok(Err(error)) => {
                self.clear_pending_auth(mcp_name).await;
                return Err(crate::Error::message(error));
            }
            Err(_) => {
                self.clear_pending_auth(mcp_name).await;
                return Err(crate::Error::message("OAuth callback channel closed"));
            }
        };

        let remote_url = match self.require_mcp_config(mcp_name).await? {
            Info::Remote(remote) => remote.url,
            _ => {
                self.clear_pending_auth(mcp_name).await;
                return Err(crate::Error::message("MCP server is not remote"));
            }
        };
        let stored_state = self
            .auth
            .get_oauth_state_for_url(mcp_name, &remote_url)
            .await?;
        if stored_state.as_deref() != Some(&result.oauth_state) {
            self.clear_pending_auth(mcp_name).await;
            return Err(crate::Error::message(
                "OAuth state mismatch - potential CSRF attack",
            ));
        }
        let _ = self.auth.clear_oauth_state(mcp_name).await;
        self.finish_auth(mcp_name, &code).await
    }

    /// From reference `MCP.finishAuth`.
    pub async fn finish_auth(&self, mcp_name: &str, authorization_code: &str) -> Result<Status> {
        self.require_mcp_config(mcp_name).await?;
        let pending = self
            .pending_oauth
            .lock()
            .await
            .get(mcp_name)
            .cloned()
            .ok_or_else(|| {
                crate::Error::message(format!("No pending OAuth flow for MCP server: {mcp_name}"))
            })?;

        if let Err(error) = pending.transport.finish_auth(authorization_code).await {
            return Ok(Status::Failed {
                error: format!("OAuth completion failed: {error}"),
            });
        }
        if let Some(provider) = &pending.provider {
            provider.commit().await?;
        }
        self.pending_oauth.lock().await.shift_remove(mcp_name);
        let _ = self.auth.clear_code_verifier(mcp_name).await;
        let _ = self.auth.clear_oauth_state(mcp_name).await;

        let mcp_config = self.require_mcp_config(mcp_name).await?;
        let mut enabled = mcp_config.clone();
        enabled.set_enabled(true);
        self.create_and_store(mcp_name, &enabled).await
    }

    /// From reference `MCP.removeAuth`.
    pub async fn remove_auth(&self, mcp_name: &str) -> Result<()> {
        self.auth.remove(mcp_name).await?;
        oauth_callback::cancel_pending(mcp_name).await;
        self.pending_oauth.lock().await.shift_remove(mcp_name);
        Ok(())
    }

    /// From reference `MCP.supportsOAuth`.
    pub async fn supports_oauth(&self, mcp_name: &str) -> Result<bool> {
        let config = self.require_mcp_config(mcp_name).await?;
        Ok(matches!(&config, Info::Remote(remote) if remote.oauth_enabled()))
    }

    /// From reference `MCP.hasStoredTokens`.
    pub async fn has_stored_tokens(&self, mcp_name: &str) -> Result<bool> {
        let entry = self.auth.get(mcp_name).await?;
        Ok(entry.and_then(|entry| entry.tokens).is_some())
    }

    /// From reference `MCP.getAuthStatus`.
    pub async fn get_auth_status(&self, mcp_name: &str) -> Result<AuthStatus> {
        let mcp_config = self.get_mcp_config(mcp_name).await;
        let Some(Info::Remote(remote)) = mcp_config else {
            return Ok(AuthStatus::NotAuthenticated);
        };
        let entry = self.auth.get_for_url(mcp_name, &remote.url).await?;
        let Some(tokens) = entry.and_then(|entry| entry.tokens) else {
            return Ok(AuthStatus::NotAuthenticated);
        };
        if let Some(expires_at) = tokens.expires_at {
            if expires_at < now_seconds() {
                return Ok(AuthStatus::Expired);
            }
        }
        Ok(AuthStatus::Authenticated)
    }

    /// Close every connected client (mirrors the state finalizer).
    pub async fn close_all(&self) {
        let clients: Vec<Arc<Client>> = self
            .state
            .write()
            .await
            .clients
            .drain(..)
            .map(|(_, client)| client)
            .collect();
        for client in clients {
            let _ = client.close().await;
        }
        let pending: Vec<PendingOAuth> = self
            .pending_oauth
            .lock()
            .await
            .drain(..)
            .map(|(_, pending)| pending)
            .collect();
        for pending in pending {
            let _ = pending.transport.close().await;
        }
        oauth_callback::stop().await;
    }

    async fn clear_pending_auth(&self, mcp_name: &str) {
        self.pending_oauth.lock().await.shift_remove(mcp_name);
        let _ = self.auth.clear_oauth_state(mcp_name).await;
        let _ = self.auth.clear_code_verifier(mcp_name).await;
    }

    // --- internals ---

    async fn create(&self, key: &str, mcp: &Info) -> CreateResult {
        if !mcp.enabled() {
            return CreateResult {
                client: None,
                status: Status::Disabled,
                defs: None,
                instructions: None,
            };
        }

        let (client, status) = match mcp {
            Info::Local(local) => self.connect_local(key, local).await,
            Info::Remote(remote) => self.connect_remote(key, remote).await,
        };

        let Some(client) = client else {
            if !matches!(status, Status::Connected | Status::Disabled) {
                warn!(
                    message = "server unavailable",
                    key = tracing::field::display(key),
                    status = tracing::field::debug(&status)
                );
            }
            return CreateResult {
                client: None,
                status,
                defs: None,
                instructions: None,
            };
        };

        let has_tools = client
            .get_server_capabilities()
            .await
            .map(|capabilities| capabilities.has_tools())
            .unwrap_or(false);
        let listed = if has_tools {
            match catalog::defs(client.clone(), mcp.timeout()).await {
                Ok(Some(defs)) => defs,
                _ => {
                    let _ = client.close().await;
                    return CreateResult {
                        client: None,
                        status: Status::Failed {
                            error: "Failed to get tools".into(),
                        },
                        defs: None,
                        instructions: None,
                    };
                }
            }
        } else {
            Vec::new()
        };
        let instructions = client
            .get_instructions()
            .await
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());

        CreateResult {
            client: Some(client),
            status,
            defs: Some(listed),
            instructions,
        }
    }

    async fn connect_local(&self, key: &str, mcp: &Local) -> (Option<Arc<Client>>, Status) {
        let command = match mcp.command.first() {
            Some(command) => command.clone(),
            None => {
                return (
                    None,
                    Status::Failed {
                        error: format!("MCP server \"{key}\" has no command"),
                    },
                );
            }
        };
        let args = mcp.command[1..].to_vec();
        let cwd = match &mcp.cwd {
            Some(cwd) => self.directory.join(cwd),
            None => self.directory.clone(),
        };

        let mut env: Vec<(String, String)> = std::env::vars().collect();
        if command == "opencode" {
            env.retain(|(key, _)| key != "BUN_BE_BUN");
            env.push(("BUN_BE_BUN".into(), "1".into()));
        }
        if let Some(environment) = &mcp.environment {
            for (env_key, value) in environment {
                env.retain(|(existing_key, _)| existing_key != env_key);
                env.push((env_key.clone(), value.clone()));
            }
        }

        let transport = StdioTransport::new(command, args, cwd, env);
        let timeout = mcp.timeout.unwrap_or(DEFAULT_TIMEOUT);
        match self.connect_transport(Arc::new(transport), timeout).await {
            Ok(client) => (Some(client), Status::Connected),
            Err(error) => {
                warn!(
                    message = "server unavailable",
                    key = tracing::field::display(key),
                    error = tracing::field::display(&error)
                );
                (
                    None,
                    Status::Failed {
                        error: error.to_string(),
                    },
                )
            }
        }
    }

    async fn connect_remote(&self, key: &str, mcp: &Remote) -> (Option<Arc<Client>>, Status) {
        let oauth_disabled = !mcp.oauth_enabled();
        let oauth_config = mcp.oauth_config();
        let url = match Url::parse(&mcp.url) {
            Ok(url) => url,
            Err(_) => {
                return (
                    None,
                    Status::Failed {
                        error: format!("Invalid MCP URL for \"{key}\""),
                    },
                );
            }
        };

        let auth_provider: Option<Arc<dyn OAuthClientProvider>> = if oauth_disabled {
            None
        } else {
            Some(Arc::new(McpOAuthProvider::new(
                key,
                &mcp.url,
                McpOAuthConfig::from_config(oauth_config),
                McpOAuthCallbacks::default(),
                Arc::clone(&self.auth),
            )))
        };

        let headers = mcp.headers.clone();
        let connect_timeout = mcp.timeout.unwrap_or(DEFAULT_TIMEOUT);
        let mut last_status: Option<Status> = None;

        let transports: Vec<(&str, Arc<dyn Transport>)> = vec![
            (
                "StreamableHTTP",
                Arc::new(StreamableHTTPClientTransport::new(
                    url.clone(),
                    headers.clone(),
                    auth_provider.clone(),
                )),
            ),
            (
                "SSE",
                Arc::new(SSEClientTransport::new(
                    url.clone(),
                    headers.clone(),
                    auth_provider.clone(),
                )),
            ),
        ];

        for (_transport_name, transport) in transports {
            match self
                .connect_transport(Arc::clone(&transport), connect_timeout)
                .await
            {
                Ok(client) => return (Some(client), Status::Connected),
                Err(error) => {
                    let message = error.to_string();
                    let is_auth_error = error.is_unauthorized()
                        || (auth_provider.is_some() && message.contains("OAuth"));
                    if is_auth_error {
                        if message.contains("registration") || message.contains("client_id") {
                            let status = Status::NeedsClientRegistration {
                                error: "Server does not support dynamic client registration. Please provide clientId in config.".into(),
                            };
                            self.pending_oauth.lock().await.shift_remove(key);
                            return (None, status);
                        }
                        self.pending_oauth.lock().await.insert(
                            key.to_string(),
                            PendingOAuth {
                                transport,
                                provider: None,
                            },
                        );
                        return (None, Status::NeedsAuth);
                    }
                    last_status = Some(Status::Failed { error: message });
                }
            }
        }

        (
            None,
            last_status.unwrap_or(Status::Failed {
                error: "Unknown error".into(),
            }),
        )
    }

    async fn connect_transport(
        &self,
        transport: Arc<dyn Transport>,
        timeout: u64,
    ) -> Result<Arc<Client>> {
        let client =
            Client::spawn(transport, self.client_info.clone(), client_capabilities()).await?;
        register_roots_handler(&client, &self.directory).await;
        crate::util::with_timeout(client.initialize(timeout), timeout, "initialize").await?;
        Ok(client)
    }

    async fn create_and_store(&self, name: &str, mcp: &Info) -> Result<Status> {
        let result = self.create(name, mcp).await;
        let Some(client) = result.client else {
            let mut state = self.state.write().await;
            state.status.insert(name.to_string(), result.status.clone());
            if let Some(previous) = state.clients.shift_remove(name) {
                let _ = previous.close().await;
            }
            state.defs.shift_remove(name);
            state.instructions.shift_remove(name);
            return Ok(result.status);
        };
        let defs = result.defs.unwrap_or_default();
        self.store_client(name, client, defs, result.instructions, mcp.timeout())
            .await
    }

    async fn store_client(
        &self,
        name: &str,
        client: Arc<Client>,
        listed: Vec<Tool>,
        instructions: Option<String>,
        timeout: Option<u64>,
    ) -> Result<Status> {
        let previous = {
            let mut state = self.state.write().await;
            let previous = state.clients.get(name).cloned();
            state.status.insert(name.to_string(), Status::Connected);
            state.clients.insert(name.to_string(), client.clone());
            state.defs.insert(name.to_string(), listed);
            match instructions {
                Some(instructions) => {
                    state.instructions.insert(name.to_string(), instructions);
                }
                None => {
                    state.instructions.shift_remove(name);
                }
            }
            previous
        };
        self.watch(name, &client, timeout).await;
        if let Some(previous) = previous {
            if !Arc::ptr_eq(&previous, &client) {
                let _ = previous.close().await;
            }
        }
        Ok(Status::Connected)
    }

    /// Wire up `onclose`, `notifications/message` logging, and
    /// `notifications/tools/list_changed` handling. From reference `MCP.watch`.
    async fn watch(&self, name: &str, client: &Arc<Client>, timeout: Option<u64>) {
        let state = self.state.clone();
        let events = self.events.clone();
        let watch_name = name.to_string();
        let watched_client = client.clone();
        client
            .set_onclose(move || {
                let state = Arc::clone(&state);
                let events = events.clone();
                let name = watch_name.clone();
                let watched_client = Arc::clone(&watched_client);
                tokio::spawn(async move {
                    let mut guard = state.write().await;
                    if !guard
                        .clients
                        .get(&name)
                        .map(|client| Arc::ptr_eq(client, &watched_client))
                        .unwrap_or(false)
                    {
                        return;
                    }
                    guard.clients.shift_remove(&name);
                    guard.defs.shift_remove(&name);
                    guard.instructions.shift_remove(&name);
                    guard.status.insert(
                        name.clone(),
                        Status::Failed {
                            error: "Connection closed".into(),
                        },
                    );
                    warn!(
                        message = "MCP connection closed",
                        server = tracing::field::display(&name)
                    );
                    if let Some(events) = events {
                        let _ = events.send(McpEvent::ToolsChanged { server: name });
                    }
                });
            })
            .await;

        client
            .set_notification_handler("notifications/message", {
                let server = name.to_string();
                Arc::new(move |params: Option<serde_json::Value>| {
                    if let Ok(params) = serde_json::from_value::<LoggingMessageNotificationParams>(
                        params.unwrap_or(serde_json::Value::Null),
                    ) {
                        server_log(&server, params);
                    }
                })
            })
            .await;

        if !client
            .get_server_capabilities()
            .await
            .map(|capabilities| capabilities.has_tools())
            .unwrap_or(false)
        {
            return;
        }

        let state = self.state.clone();
        let events = self.events.clone();
        let name = name.to_string();
        let watched_client = client.clone();
        client
            .set_notification_handler("notifications/tools/list_changed", {
                let client = Arc::clone(client);
                Arc::new(move |_params: Option<serde_json::Value>| {
                    let client = Arc::clone(&client);
                    let state = Arc::clone(&state);
                    let events = events.clone();
                    let name = name.clone();
                    let watched_client = Arc::clone(&watched_client);
                    tokio::spawn(async move {
                        let Ok(Some(defs)) = catalog::defs(client, timeout).await else {
                            return;
                        };
                        let mut guard = state.write().await;
                        let connected = guard
                            .clients
                            .get(&name)
                            .map(|stored| Arc::ptr_eq(stored, &watched_client))
                            .unwrap_or(false);
                        let is_connected = guard
                            .status
                            .get(&name)
                            .map(|status| status.is_connected())
                            .unwrap_or(false);
                        if connected && is_connected {
                            guard.defs.insert(name.clone(), defs);
                            if let Some(events) = events {
                                let _ = events.send(McpEvent::ToolsChanged { server: name });
                            }
                        }
                    });
                })
            })
            .await;
    }

    async fn collect_from_connected<T, LF, F>(
        &self,
        label: &str,
        key: Option<&catalog::KeyFn<T>>,
        list: LF,
        target_client_name: Option<&str>,
    ) -> Result<IndexMap<String, serde_json::Value>>
    where
        T: crate::catalog::Named + Serialize + Send + Sync,
        LF: Fn(Arc<Client>, Option<u64>) -> F + Send + Sync,
        F: std::future::Future<Output = crate::Result<Vec<T>>> + Send,
    {
        let connected = {
            let state = self.state.read().await;
            let mut items = Vec::new();
            for (name, client) in &state.clients {
                let is_connected = state
                    .status
                    .get(name)
                    .map(|status| status.is_connected())
                    .unwrap_or(false);
                if !is_connected {
                    continue;
                }
                if let Some(target) = target_client_name {
                    if name != target {
                        continue;
                    }
                }
                let timeout = request_timeout(state.config.get(name), self.default_timeout);
                items.push((name.clone(), client.clone(), timeout));
            }
            items
        };

        let list = Arc::new(list);
        let mut futures = Vec::new();
        for (client_name, client, timeout) in connected {
            let list = Arc::clone(&list);
            futures.push(async move {
                catalog::fetch(
                    &client_name,
                    client,
                    |client: &Arc<Client>| list(client.clone(), timeout),
                    label,
                    key,
                )
                .await
            });
        }
        let results = futures::future::join_all(futures).await;

        let mut map = IndexMap::new();
        for result in results {
            if let Ok(Some(items)) = result {
                map.extend(items);
            }
        }
        Ok(map)
    }

    async fn with_client<A, F, Fut>(
        &self,
        client_name: &str,
        label: &str,
        f: F,
    ) -> Result<Option<A>>
    where
        F: FnOnce(Arc<Client>, u64) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = crate::Result<A>> + Send,
        A: Send + 'static,
    {
        let (client, timeout) = {
            let state = self.state.read().await;
            let client = state.clients.get(client_name).cloned();
            let timeout = request_timeout(state.config.get(client_name), self.default_timeout);
            (client, timeout)
        };
        let Some(client) = client else {
            warn!(
                message = "client not found for",
                label = tracing::field::display(label),
                client_name = tracing::field::debug(client_name)
            );
            return Ok(None);
        };
        match f(client, timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT)).await {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                error!(
                    message = "failed to",
                    label = tracing::field::display(label),
                    client_name = tracing::field::debug(client_name),
                    error = tracing::field::display(&error)
                );
                Ok(None)
            }
        }
    }

    async fn get_mcp_config(&self, name: &str) -> Option<Info> {
        self.state.read().await.config.get(name).cloned()
    }

    async fn require_mcp_config(&self, name: &str) -> Result<Info> {
        self.get_mcp_config(name)
            .await
            .ok_or_else(|| crate::Error::message(format!("MCP server not found: {name}")))
    }
}

fn request_timeout(config: Option<&Info>, default_timeout: Option<u64>) -> Option<u64> {
    config.and_then(Info::timeout).or(default_timeout)
}

fn server_log(server: &str, params: LoggingMessageNotificationParams) {
    let level = tracing::field::debug(&params.level);
    let logger = tracing::field::debug(&params.logger);
    let data = tracing::field::debug(&params.data);
    match params.level {
        crate::types::LoggingLevel::Debug => {
            debug!(
                message = "MCP server log",
                server = server,
                logger = logger,
                level = level,
                data = data
            )
        }
        crate::types::LoggingLevel::Info | crate::types::LoggingLevel::Notice => {
            info!(
                message = "MCP server log",
                server = server,
                logger = logger,
                level = level,
                data = data
            )
        }
        crate::types::LoggingLevel::Warning => {
            warn!(
                message = "MCP server log",
                server = server,
                logger = logger,
                level = level,
                data = data
            )
        }
        crate::types::LoggingLevel::Error
        | crate::types::LoggingLevel::Critical
        | crate::types::LoggingLevel::Alert
        | crate::types::LoggingLevel::Emergency => {
            error!(
                message = "MCP server log",
                server = server,
                logger = logger,
                level = level,
                data = data
            )
        }
    }
}

/// Result of `startAuth`.
pub struct AuthStartResult {
    pub authorization_url: String,
    pub oauth_state: String,
    pub client: Option<Arc<Client>>,
}
