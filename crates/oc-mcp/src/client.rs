//! MCP client (JSON-RPC request/response, initialize handshake, tool calls).
//!
//! Port of `@modelcontextprotocol/sdk@1.29.0` `client/index.js` (the `Client`
//! and `Protocol` classes) as used by `reference/packages/opencode/src/mcp/index.ts`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::{oneshot, watch, Mutex};

use crate::jsonrpc::{JsonRpcError, Message, Request, RequestId, INTERNAL_ERROR, METHOD_NOT_FOUND};
use crate::transport::{MessageReceiver, Transport};
use crate::types::{
    CallToolRequestParams, CallToolResult, CancelledNotificationParams, ClientCapabilities,
    GetPromptRequestParams, GetPromptResult, Implementation, InitializeResult, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListRootsRequestParams, ListToolsResult,
    ProgressNotificationParams, ReadResourceRequestParams, ReadResourceResult, RequestMeta,
    ServerCapabilities, SUPPORTED_PROTOCOL_VERSIONS,
};

type Result<T> = crate::Result<T>;

const CANCELLED_NOTIFICATION: &str = "notifications/cancelled";
const REQUEST_TIMED_OUT_REASON: &str = "Request timed out";

/// Default maximum number of requests that may be in flight (sent to the
/// server and awaiting a response) at once. Beyond this the caller is
/// backpressured (rejected) rather than growing the pending map unboundedly,
/// mirroring the reference client's bounded request concurrency.
const DEFAULT_MAX_INFLIGHT: usize = 64;

/// A handler for a server→client request (e.g. `roots/list`).
pub type RequestHandler =
    Arc<dyn Fn(Option<serde_json::Value>) -> Result<serde_json::Value> + Send + Sync>;

/// A handler for a server notification.
pub type NotificationHandler = Arc<dyn Fn(Option<serde_json::Value>) + Send + Sync>;

struct Pending {
    tx: oneshot::Sender<Result<serde_json::Value>>,
}

/// RAII guard that releases a request-concurrency permit on drop. Used to
/// implement backpressure without leaking permits across the many early-return
/// paths in the request methods.
struct RequestPermit(Option<tokio::sync::OwnedSemaphorePermit>);

impl Drop for RequestPermit {
    fn drop(&mut self) {
        drop(self.0.take());
    }
}

pub struct Client {
    transport: Arc<dyn Transport>,
    client_info: Implementation,
    capabilities: ClientCapabilities,
    pending: Mutex<HashMap<RequestId, Pending>>,
    /// progressToken → signal used by `call_tool` to reset its request timeout.
    progress: Mutex<HashMap<RequestId, Arc<watch::Sender<()>>>>,
    request_handlers: Mutex<HashMap<String, RequestHandler>>,
    notification_handlers: Mutex<HashMap<String, NotificationHandler>>,
    next_id: AtomicU64,
    next_progress_token: AtomicU64,
    onclose: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    server_capabilities: Mutex<Option<crate::types::ServerCapabilities>>,
    server_info: Mutex<Option<Implementation>>,
    instructions: Mutex<Option<String>>,
    initialized: AtomicBool,
    /// Bounded concurrency for requests awaiting a response. Acquired before a
    /// request is inserted into `pending` so the pending map cannot grow
    /// without bound. This is the backpressure mechanism.
    request_permits: Arc<tokio::sync::Semaphore>,
}

impl Client {
    /// Construct and connect a client: starts the transport, performs the
    /// `initialize` handshake and sends `notifications/initialized`.
    /// From reference/packages/opencode/src/mcp/index.ts (`createClient`,
    /// `connectTransport`).
    pub async fn connect(
        transport: Arc<dyn Transport>,
        client_info: Implementation,
        capabilities: ClientCapabilities,
        timeout_ms: u64,
    ) -> Result<Arc<Self>> {
        let client = Self::spawn(transport, client_info, capabilities).await?;
        client.initialize(timeout_ms).await?;
        Ok(client)
    }

    /// Start the transport and the message read loop without performing the
    /// handshake; callers may register request handlers before `initialize`.
    pub async fn spawn(
        transport: Arc<dyn Transport>,
        client_info: Implementation,
        capabilities: ClientCapabilities,
    ) -> Result<Arc<Self>> {
        Self::spawn_with_max_inflight(transport, client_info, capabilities, DEFAULT_MAX_INFLIGHT)
            .await
    }

    /// Like [`Client::spawn`] but with an explicit bound on the number of
    /// requests that may be awaiting a response at once. Requests beyond the
    /// bound are rejected once the ready permit is not available within the
    /// request's own timeout. Exposed for backpressure testing.
    pub async fn spawn_with_max_inflight(
        transport: Arc<dyn Transport>,
        client_info: Implementation,
        capabilities: ClientCapabilities,
        max_inflight: usize,
    ) -> Result<Arc<Self>> {
        let client = Arc::new(Client {
            transport,
            client_info,
            capabilities,
            pending: Mutex::new(HashMap::new()),
            progress: Mutex::new(HashMap::new()),
            request_handlers: Mutex::new(HashMap::new()),
            notification_handlers: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            next_progress_token: AtomicU64::new(1),
            onclose: Mutex::new(None),
            server_capabilities: Mutex::new(None),
            server_info: Mutex::new(None),
            instructions: Mutex::new(None),
            initialized: AtomicBool::new(false),
            request_permits: Arc::new(tokio::sync::Semaphore::new(max_inflight)),
        });

        let receiver = client.transport.start().await?;
        client.spawn_read_loop(receiver);
        Ok(client)
    }

    /// The `initialize` request/response exchange plus the
    /// `notifications/initialized` follow-up. From the SDK's `_initialize`.
    pub async fn initialize(self: &Arc<Self>, timeout_ms: u64) -> Result<()> {
        let params = json!({
            "protocolVersion": crate::types::LATEST_PROTOCOL_VERSION,
            "capabilities": self.capabilities,
            "clientInfo": self.client_info,
        });
        let result = self
            .request_inner(
                "initialize",
                Some(params),
                timeout_ms,
                None,
                false,
                REQUEST_TIMED_OUT_REASON,
            )
            .await?;
        let initialize_result: InitializeResult = serde_json::from_value(result)?;

        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&initialize_result.protocol_version.as_str()) {
            return Err(crate::Error::message(format!(
                "Server's protocol version is not supported: {}",
                initialize_result.protocol_version
            )));
        }
        *self.server_capabilities.lock().await = Some(initialize_result.capabilities);
        *self.server_info.lock().await = Some(initialize_result.server_info);
        *self.instructions.lock().await = initialize_result.instructions;
        self.transport
            .set_protocol_version(crate::types::LATEST_PROTOCOL_VERSION.to_string())
            .await;

        self.notification("notifications/initialized", None).await?;
        self.initialized.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_read_loop(self: &Arc<Self>, mut receiver: MessageReceiver) {
        let client = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    Message::Response(response) => {
                        let pending = client.pending.lock().await.remove(&response.id);
                        if let Some(pending) = pending {
                            let _ = pending.tx.send(Ok(response.result));
                        }
                    }
                    Message::Error(response) => {
                        let pending = client.pending.lock().await.remove(&response.id);
                        if let Some(pending) = pending {
                            let _ = pending.tx.send(Err(crate::Error::Rpc {
                                code: response.error.code,
                                message: response.error.message,
                            }));
                        }
                    }
                    Message::Request(request) => {
                        let client = Arc::clone(&client);
                        tokio::spawn(async move {
                            let response = handle_server_request(&client, request).await;
                            let _ = client.transport.send(response).await;
                        });
                    }
                    Message::Notification(notification) => {
                        dispatch_notification(&client, &notification.method, notification.params)
                            .await;
                    }
                }
            }

            let onclose = client.onclose.lock().await.take();
            if let Some(callback) = onclose {
                callback();
            }
            let pending = std::mem::take(&mut *client.pending.lock().await);
            for (_, pending) in pending {
                let _ = pending
                    .tx
                    .send(Err(crate::Error::message("Connection closed")));
            }
        });
    }

    /// Send a request and await its result, resolving JSON-RPC errors.
    /// From the SDK `Protocol.request`.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
        timeout_ms: u64,
    ) -> Result<serde_json::Value> {
        self.request_inner(
            method,
            params,
            timeout_ms,
            None,
            true,
            REQUEST_TIMED_OUT_REASON,
        )
        .await
    }

    /// Send a request that can be cancelled by an arbitrary caller-owned
    /// future. Once cancellation wins, the pending waiter is removed and MCP
    /// `notifications/cancelled` is sent with the request id and reason.
    ///
    /// `initialize` is never cancelled on the wire, as required by MCP.
    pub async fn request_cancellable<C>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
        timeout_ms: u64,
        cancellation: C,
        reason: impl Into<String>,
    ) -> Result<serde_json::Value>
    where
        C: Future<Output = ()> + Send,
    {
        let reason = reason.into();
        self.request_inner(
            method,
            params,
            timeout_ms,
            Some(Box::pin(cancellation)),
            true,
            &reason,
        )
        .await
    }

    async fn request_inner(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
        timeout_ms: u64,
        cancellation: Option<crate::util::BoxFuture<'_, ()>>,
        cancel_on_timeout: bool,
        cancellation_reason: &str,
    ) -> Result<serde_json::Value> {
        // Backpressure: acquire a permit before allocating an id or inserting
        // into `pending`. This bounds the number of requests awaiting a
        // response to `max_inflight`; callers beyond the bound are rejected
        // once the permit is not available within the request timeout rather
        // than growing `pending` without bound.
        let permit = match tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            self.request_permits.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => RequestPermit(Some(permit)),
            Ok(Err(_)) => return Err(crate::Error::message("MCP client is closed")),
            Err(_) => {
                return Err(crate::Error::message(
                    "MCP request concurrency limit reached; request rejected (backpressure)",
                ));
            }
        };

        let id = RequestId::Number(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = oneshot::channel();
        let message = Message::request(id.clone(), method, params);
        self.pending.lock().await.insert(id.clone(), Pending { tx });
        if let Err(error) = self.transport.send(message).await {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }

        let result = if let Some(mut cancellation) = cancellation {
            tokio::select! {
                result = rx => result.unwrap_or_else(|_| Err(crate::Error::message("request cancelled"))),
                _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
                    self.cancel_pending_request(
                        &id,
                        cancellation_reason,
                        cancel_on_timeout && method != "initialize",
                    ).await;
                    Err(crate::Error::Timeout {
                        ms: timeout_ms,
                        label: method.to_string(),
                    })
                }
                _ = &mut cancellation => {
                    self.cancel_pending_request(
                        &id,
                        cancellation_reason,
                        cancel_on_timeout && method != "initialize",
                    ).await;
                    Err(crate::Error::message("request cancelled"))
                }
            }
        } else {
            match tokio::time::timeout(Duration::from_millis(timeout_ms), rx).await {
                Err(_) => {
                    self.cancel_pending_request(
                        &id,
                        cancellation_reason,
                        cancel_on_timeout && method != "initialize",
                    )
                    .await;
                    Err(crate::Error::Timeout {
                        ms: timeout_ms,
                        label: method.to_string(),
                    })
                }
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(crate::Error::message("request cancelled")),
            }
        };

        // Permit released on drop.
        let _ = permit;
        result
    }

    async fn cancel_pending_request(&self, id: &RequestId, reason: &str, notify: bool) -> bool {
        let removed = self.pending.lock().await.remove(id).is_some();
        if removed && notify {
            let params = CancelledNotificationParams {
                request_id: id.clone(),
                reason: Some(reason.to_string()),
            };
            // Cancellation is best-effort. The transport's normal send path
            // remains responsible for HTTP auth/session recovery.
            let _ = self
                .notification(
                    CANCELLED_NOTIFICATION,
                    Some(serde_json::to_value(params).expect("cancellation params serialize")),
                )
                .await;
        }
        removed
    }

    /// Fire-and-forget notification. From the SDK `Protocol.notification`.
    pub async fn notification(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        self.transport
            .send(Message::notification(method, params))
            .await
    }

    /// Register a handler for a server→client request.
    pub async fn set_request_handler(&self, method: &str, handler: RequestHandler) {
        self.request_handlers
            .lock()
            .await
            .insert(method.to_string(), handler);
    }

    /// Register a handler for a server notification.
    pub async fn set_notification_handler(&self, method: &str, handler: NotificationHandler) {
        self.notification_handlers
            .lock()
            .await
            .insert(method.to_string(), handler);
    }

    /// Register the close callback invoked when the transport dies.
    pub async fn set_onclose(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self.onclose.lock().await = Some(Arc::new(callback));
    }

    pub async fn get_server_capabilities(&self) -> Option<ServerCapabilities> {
        self.server_capabilities.lock().await.clone()
    }

    pub async fn get_server_info(&self) -> Option<Implementation> {
        self.server_info.lock().await.clone()
    }

    pub async fn get_instructions(&self) -> Option<String> {
        self.instructions.lock().await.clone()
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }

    pub fn pid(&self) -> Option<u32> {
        self.transport.pid()
    }

    /// `tools/list`, paging via `cursor` (SDK `Client.listTools`).
    pub async fn list_tools(
        &self,
        cursor: Option<String>,
        timeout_ms: u64,
    ) -> Result<ListToolsResult> {
        let params = cursor.map(|cursor| json!({ "cursor": cursor }));
        let value = self.request("tools/list", params, timeout_ms).await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `prompts/list`.
    pub async fn list_prompts(
        &self,
        cursor: Option<String>,
        timeout_ms: u64,
    ) -> Result<ListPromptsResult> {
        let params = cursor.map(|cursor| json!({ "cursor": cursor }));
        let value = self.request("prompts/list", params, timeout_ms).await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `resources/list`.
    pub async fn list_resources(
        &self,
        cursor: Option<String>,
        timeout_ms: u64,
    ) -> Result<ListResourcesResult> {
        let params = cursor.map(|cursor| json!({ "cursor": cursor }));
        let value = self.request("resources/list", params, timeout_ms).await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `resources/templates/list`.
    pub async fn list_resource_templates(
        &self,
        cursor: Option<String>,
        timeout_ms: u64,
    ) -> Result<ListResourceTemplatesResult> {
        let params = cursor.map(|cursor| json!({ "cursor": cursor }));
        let value = self
            .request("resources/templates/list", params, timeout_ms)
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `prompts/get`.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
        timeout_ms: u64,
    ) -> Result<GetPromptResult> {
        let params = GetPromptRequestParams {
            name: name.to_string(),
            arguments,
        };
        let value = self
            .request(
                "prompts/get",
                Some(serde_json::to_value(params)?),
                timeout_ms,
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `resources/read`.
    pub async fn read_resource(&self, uri: &str, timeout_ms: u64) -> Result<ReadResourceResult> {
        let params = ReadResourceRequestParams {
            uri: uri.to_string(),
        };
        let value = self
            .request(
                "resources/read",
                Some(serde_json::to_value(params)?),
                timeout_ms,
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// `tools/call` with progress support. Mirrors `McpCatalog.convertTool`'s
    /// transport call in the reference: an `_meta.progressToken` is sent and
    /// `notifications/progress` resets the request timeout.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        timeout_ms: u64,
    ) -> Result<CallToolResult> {
        self.call_tool_inner(name, arguments, timeout_ms, None, REQUEST_TIMED_OUT_REASON)
            .await
    }

    /// `tools/call` with caller-driven cancellation in addition to its
    /// timeout and progress-reset behavior.
    pub async fn call_tool_cancellable<C>(
        &self,
        name: &str,
        arguments: serde_json::Value,
        timeout_ms: u64,
        cancellation: C,
        reason: impl Into<String>,
    ) -> Result<CallToolResult>
    where
        C: Future<Output = ()> + Send,
    {
        let reason = reason.into();
        self.call_tool_inner(
            name,
            arguments,
            timeout_ms,
            Some(Box::pin(cancellation)),
            &reason,
        )
        .await
    }

    async fn call_tool_inner(
        &self,
        name: &str,
        arguments: serde_json::Value,
        timeout_ms: u64,
        mut cancellation: Option<crate::util::BoxFuture<'_, ()>>,
        cancellation_reason: &str,
    ) -> Result<CallToolResult> {
        // Backpressure: bound the tools/call concurrency to `max_inflight`.
        // The RAII guard releases the permit on every exit path.
        let permit = match tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            self.request_permits.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => RequestPermit(Some(permit)),
            Ok(Err(_)) => return Err(crate::Error::message("MCP client is closed")),
            Err(_) => {
                return Err(crate::Error::message(
                    "MCP request concurrency limit reached; request rejected (backpressure)",
                ));
            }
        };
        let _ = permit;

        let progress_token =
            RequestId::Number(self.next_progress_token.fetch_add(1, Ordering::SeqCst));
        let (reset_tx, mut reset_rx) = watch::channel(());
        self.progress
            .lock()
            .await
            .insert(progress_token.clone(), Arc::new(reset_tx));

        let params = CallToolRequestParams {
            name: name.to_string(),
            arguments,
            meta: Some(RequestMeta {
                progress_token: Some(progress_token.clone()),
            }),
        };
        let id = RequestId::Number(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, mut rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), Pending { tx });
        if let Err(error) = self
            .transport
            .send(Message::request(
                id.clone(),
                "tools/call",
                Some(serde_json::to_value(&params)?),
            ))
            .await
        {
            self.pending.lock().await.remove(&id);
            self.progress.lock().await.remove(&progress_token);
            return Err(error);
        }

        let outcome = loop {
            let deadline = tokio::time::sleep(Duration::from_millis(timeout_ms));
            tokio::pin!(deadline);
            if let Some(cancel) = cancellation.as_mut() {
                tokio::select! {
                    _ = &mut deadline => {
                        self.cancel_pending_request(&id, cancellation_reason, true).await;
                        self.progress.lock().await.remove(&progress_token);
                        return Err(crate::Error::Timeout {
                            ms: timeout_ms,
                            label: format!("tools/call {name}"),
                        });
                    }
                    changed = reset_rx.changed() => {
                        if changed.is_err() {
                            self.pending.lock().await.remove(&id);
                            self.progress.lock().await.remove(&progress_token);
                            return Err(crate::Error::message("progress channel closed"));
                        }
                    }
                    _ = cancel => {
                        self.cancel_pending_request(&id, cancellation_reason, true).await;
                        self.progress.lock().await.remove(&progress_token);
                        return Err(crate::Error::message("request cancelled"));
                    }
                    result = &mut rx => break result,
                }
            } else {
                tokio::select! {
                    _ = &mut deadline => {
                        self.cancel_pending_request(&id, cancellation_reason, true).await;
                        self.progress.lock().await.remove(&progress_token);
                        return Err(crate::Error::Timeout {
                            ms: timeout_ms,
                            label: format!("tools/call {name}"),
                        });
                    }
                    changed = reset_rx.changed() => {
                        if changed.is_err() {
                            self.pending.lock().await.remove(&id);
                            self.progress.lock().await.remove(&progress_token);
                            return Err(crate::Error::message("progress channel closed"));
                        }
                    }
                    result = &mut rx => break result,
                }
            }
        };
        self.progress.lock().await.remove(&progress_token);

        match outcome {
            Ok(Ok(value)) => Ok(serde_json::from_value(value)?),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(crate::Error::message("request cancelled")),
        }
    }

    /// Close the transport (SDK `Client.close`).
    pub async fn close(&self) -> Result<()> {
        self.transport.close().await
    }
}

async fn handle_server_request(client: &Arc<Client>, request: Request) -> Message {
    if request.method == "ping" {
        return Message::response(request.id, json!({}));
    }
    let handler = client
        .request_handlers
        .lock()
        .await
        .get(&request.method)
        .cloned();
    match handler {
        Some(handler) => match handler(request.params) {
            Ok(result) => Message::response(request.id, result),
            Err(error) => Message::error_response(
                request.id,
                JsonRpcError {
                    code: INTERNAL_ERROR,
                    message: error.to_string(),
                    data: None,
                },
            ),
        },
        None => Message::error_response(
            request.id,
            JsonRpcError {
                code: METHOD_NOT_FOUND,
                message: "Method not found".into(),
                data: None,
            },
        ),
    }
}

async fn dispatch_notification(
    client: &Arc<Client>,
    method: &str,
    params: Option<serde_json::Value>,
) {
    if method == "notifications/progress" {
        if let Some(params) = params {
            if let Ok(params) = serde_json::from_value::<ProgressNotificationParams>(params) {
                if let Some(reset) = client
                    .progress
                    .lock()
                    .await
                    .get(&params.progress_token)
                    .cloned()
                {
                    let _ = reset.send(());
                }
            }
        }
        return;
    }
    let handler = client
        .notification_handlers
        .lock()
        .await
        .get(method)
        .cloned();
    if let Some(handler) = handler {
        handler(params);
    }
}

/// Register the `roots/list` request handler that serves the workspace
/// directory as a root. From reference/packages/opencode/src/mcp/index.ts
/// (`createClient`).
pub async fn register_roots_handler(client: &Arc<Client>, directory: &std::path::Path) {
    let directory = directory.to_path_buf();
    client
        .set_request_handler(
            "roots/list",
            Arc::new(move |params: Option<serde_json::Value>| {
                let _params: Option<ListRootsRequestParams> = params
                    .as_ref()
                    .and_then(|value| serde_json::from_value(value.clone()).ok());
                Ok(json!({
                    "roots": [{ "uri": crate::util::path_to_file_url(&directory) }]
                }))
            }),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_token_params_are_number_ids() {
        let params = serde_json::to_value(CallToolRequestParams {
            name: "x".into(),
            arguments: json!({}),
            meta: Some(RequestMeta {
                progress_token: Some(RequestId::Number(7)),
            }),
        })
        .unwrap();
        assert_eq!(params["_meta"]["progressToken"], 7);
    }
}
