//! A small, real Language Server Protocol process client.
//!
//! The client deliberately owns only the LSP process boundary.  It does not
//! depend on `oc-server`, the tool registry, or the session runner, so those
//! layers can opt into it without making the project crate depend on the
//! server graph.  Messages use the LSP/JSON-RPC `Content-Length` framing and
//! responses are correlated by the numeric request id assigned by the client.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, Notify};

const DEFAULT_MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const INBOUND_EVENT_CAPACITY: usize = 64;

/// Errors produced by the language-server process boundary.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum LspError {
    #[error("failed to spawn language server `{program}`: {message}")]
    Spawn { program: String, message: String },

    #[error("language server is unavailable: {0}")]
    Unavailable(String),

    #[error("language server process exited with code {code:?}: {stderr}")]
    ProcessExited { code: Option<i32>, stderr: String },

    #[error("language server I/O error: {0}")]
    Io(String),

    #[error("invalid language server JSON-RPC message: {0}")]
    Protocol(String),

    #[error("language server returned JSON-RPC error {code}: {message}")]
    Server {
        code: i64,
        message: String,
        data: Option<Value>,
    },

    #[error("language server request timed out after {0:?}")]
    Timeout(Duration),

    #[error("language server response for request {0} has no result")]
    MissingResult(u64),

    #[error("LSP operation `{0}` requires a server-specific parameter; use request_method")]
    UnsupportedOperation(&'static str),
}

/// Process and protocol settings for [`LspClient`].
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub root_uri: Option<String>,
    pub initialization_options: Option<Value>,
    pub request_timeout: Duration,
    pub max_message_bytes: usize,
}

impl LspServerConfig {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            root_uri: None,
            initialization_options: None,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
        }
    }

    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub fn with_max_message_bytes(mut self, max_message_bytes: usize) -> Self {
        self.max_message_bytes = max_message_bytes.max(1);
        self
    }
}

#[derive(Debug)]
enum Outbound {
    Json(Value),
}

/// A message initiated by the language server.
///
/// Notifications are delivered for consumers that need diagnostics, logs, or
/// progress updates. Requests are also surfaced before the client sends the
/// standard `MethodNotFound` response; callers that do not implement a server
/// request can therefore keep the protocol flowing without losing observability.
#[derive(Debug, Clone, PartialEq)]
pub enum LspEvent {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

struct Shared {
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, LspError>>>>,
    terminal: Mutex<Option<LspError>>,
    events: broadcast::Sender<LspEvent>,
    done: Notify,
}

impl Shared {
    fn new() -> Self {
        let (events, _) = broadcast::channel(INBOUND_EVENT_CAPACITY);
        Self {
            pending: Mutex::new(HashMap::new()),
            terminal: Mutex::new(None),
            events,
            done: Notify::new(),
        }
    }

    async fn terminal_error(&self) -> Option<LspError> {
        self.terminal.lock().await.clone()
    }

    async fn terminate(&self, error: LspError) {
        let error = {
            let mut terminal = self.terminal.lock().await;
            if let Some(existing) = terminal.as_ref() {
                existing.clone()
            } else {
                *terminal = Some(error.clone());
                error
            }
        };

        let pending = {
            let mut pending = self.pending.lock().await;
            std::mem::take(&mut *pending)
        };
        for (_, sender) in pending {
            let _ = sender.send(Err(error.clone()));
        }
        self.done.notify_waiters();
    }

    async fn wait_done(&self) {
        loop {
            // Register the notification before checking the state so a
            // process exit between the check and await cannot be missed.
            let notified = self.done.notified();
            if self.terminal.lock().await.is_some() {
                return;
            }
            notified.await;
        }
    }
}

struct Inner {
    shared: Arc<Shared>,
    outbound: mpsc::Sender<Outbound>,
    next_id: AtomicU64,
    request_timeout: Duration,
    initialized: AtomicBool,
    shutdown_started: AtomicBool,
    kill: StdMutex<Option<oneshot::Sender<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(mut kill) = self.kill.lock() {
            if let Some(sender) = kill.take() {
                let _ = sender.send(());
            }
        }
    }
}

/// A process-backed LSP client.
///
/// `spawn` only starts the process. Call [`LspClient::initialize`] before
/// using language-server requests. Dropping the last client instance asks the
/// supervisor to terminate the child, while [`LspClient::shutdown`] performs
/// the protocol-mandated `shutdown` request followed by the `exit`
/// notification.
#[derive(Clone)]
pub struct LspClient {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field(
                "initialized",
                &self.inner.initialized.load(Ordering::Acquire),
            )
            .field(
                "shutdown_started",
                &self.inner.shutdown_started.load(Ordering::Acquire),
            )
            .finish()
    }
}

impl LspClient {
    /// Starts a language-server process with piped stdin/stdout/stderr.
    pub async fn spawn(config: LspServerConfig) -> Result<Self, LspError> {
        if config.program.trim().is_empty() {
            return Err(LspError::Unavailable(
                "language server command is empty".to_string(),
            ));
        }

        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }

        let mut child = command.spawn().map_err(|error| LspError::Spawn {
            program: config.program.clone(),
            message: error.to_string(),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            LspError::Unavailable("language server stdin was not piped".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            LspError::Unavailable("language server stdout was not piped".to_string())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            LspError::Unavailable("language server stderr was not piped".to_string())
        })?;

        let shared = Arc::new(Shared::new());
        let (outbound, outbound_rx) = mpsc::channel(64);
        let (kill_tx, kill_rx) = oneshot::channel();

        let reader_shared = shared.clone();
        let reader_outbound = outbound.clone();
        let max_message_bytes = config.max_message_bytes.max(1);
        let reader = tokio::spawn(async move {
            reader_loop(
                BufReader::new(stdout),
                reader_shared,
                reader_outbound,
                max_message_bytes,
            )
            .await;
        });

        let writer_shared = shared.clone();
        let writer = tokio::spawn(async move {
            writer_loop(stdin, outbound_rx, writer_shared).await;
        });

        let stderr_task = tokio::spawn(read_stderr(stderr, max_message_bytes));
        tokio::spawn(supervise(
            child,
            kill_rx,
            shared.clone(),
            reader,
            writer,
            stderr_task,
        ));

        Ok(Self {
            inner: Arc::new(Inner {
                shared,
                outbound,
                next_id: AtomicU64::new(1),
                request_timeout: config.request_timeout,
                initialized: AtomicBool::new(false),
                shutdown_started: AtomicBool::new(false),
                kill: StdMutex::new(Some(kill_tx)),
            }),
        })
    }

    /// Sends an `initialize` request and the required `initialized`
    /// notification. The response must be a JSON object; a missing/null
    /// result is treated as a protocol error rather than success.
    pub async fn initialize(&self, params: Value) -> Result<Value, LspError> {
        if self.inner.initialized.swap(true, Ordering::AcqRel) {
            return Err(LspError::Protocol(
                "language server was initialized more than once".to_string(),
            ));
        }
        let result = match self.request("initialize", params).await {
            Ok(result) => result,
            Err(error) => {
                self.inner.initialized.store(false, Ordering::Release);
                return Err(error);
            }
        };
        if !result.is_object() {
            self.inner.initialized.store(false, Ordering::Release);
            return Err(LspError::Protocol(
                "initialize response result must be a JSON object".to_string(),
            ));
        }
        if let Err(error) = self.notify("initialized", json!({})).await {
            self.inner.initialized.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(result)
    }

    /// Sends one JSON-RPC request and waits for the response with the same
    /// numeric id. Server-initiated messages are published to
    /// [`LspClient::subscribe`]; server requests also receive an explicit
    /// `MethodNotFound` error until a request handler is added.
    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Value, LspError> {
        let method = method.into();
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();

        if let Some(error) = self.inner.shared.terminal_error().await {
            return Err(error);
        }
        self.inner.shared.pending.lock().await.insert(id, sender);

        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if self
            .inner
            .outbound
            .send(Outbound::Json(message))
            .await
            .is_err()
        {
            self.inner.shared.pending.lock().await.remove(&id);
            return Err(self.unavailable_error().await);
        }

        match tokio::time::timeout(self.inner.request_timeout, receiver).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(self.unavailable_error().await),
            Err(_) => {
                self.inner.shared.pending.lock().await.remove(&id);
                Err(LspError::Timeout(self.inner.request_timeout))
            }
        }
    }

    /// Sends a JSON-RPC notification. Notifications have no response and
    /// therefore never turn an unavailable process into an empty success.
    pub async fn notify(&self, method: impl Into<String>, params: Value) -> Result<(), LspError> {
        if let Some(error) = self.inner.shared.terminal_error().await {
            return Err(error);
        }
        self.inner
            .outbound
            .send(Outbound::Json(json!({
                "jsonrpc": "2.0",
                "method": method.into(),
                "params": params,
            })))
            .await
            .map_err(|_| LspError::Unavailable("language server writer stopped".to_string()))
    }

    /// Subscribes to messages initiated by the language server.
    ///
    /// The reader keeps protocol handling independent from consumers: every
    /// subscriber receives a clone of each event until its bounded buffer is
    /// overrun. Events are best-effort telemetry and never block response
    /// correlation or notification delivery.
    pub fn subscribe(&self) -> broadcast::Receiver<LspEvent> {
        self.inner.shared.events.subscribe()
    }

    /// Performs the LSP graceful shutdown handshake.
    pub async fn shutdown(&self) -> Result<(), LspError> {
        if self.inner.shutdown_started.swap(true, Ordering::AcqRel) {
            self.inner.shared.wait_done().await;
            return Ok(());
        }

        let response = self.request("shutdown", Value::Null).await;
        if let Err(error) = response {
            self.kill().await;
            return Err(error);
        }
        if let Err(error) = self.notify("exit", Value::Null).await {
            self.kill().await;
            return Err(error);
        }

        if tokio::time::timeout(self.inner.request_timeout, self.inner.shared.wait_done())
            .await
            .is_err()
        {
            self.kill().await;
            let _ = tokio::time::timeout(self.inner.request_timeout, self.inner.shared.wait_done())
                .await;
            return Err(LspError::Timeout(self.inner.request_timeout));
        }
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.inner.initialized.load(Ordering::Acquire)
    }

    async fn unavailable_error(&self) -> LspError {
        self.inner.shared.terminal_error().await.unwrap_or_else(|| {
            LspError::Unavailable("language server process is unavailable".to_string())
        })
    }

    async fn kill(&self) {
        let sender = self.inner.kill.lock().ok().and_then(|mut kill| kill.take());
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }
}

async fn writer_loop(
    mut stdin: tokio::process::ChildStdin,
    mut outbound: mpsc::Receiver<Outbound>,
    shared: Arc<Shared>,
) {
    while let Some(Outbound::Json(message)) = outbound.recv().await {
        let frame = match encode_message(&message) {
            Ok(frame) => frame,
            Err(error) => {
                shared.terminate(error).await;
                return;
            }
        };
        if let Err(error) = stdin.write_all(&frame).await {
            shared.terminate(LspError::Io(error.to_string())).await;
            return;
        }
        if let Err(error) = stdin.flush().await {
            shared.terminate(LspError::Io(error.to_string())).await;
            return;
        }
    }
}

async fn reader_loop<R>(
    mut reader: R,
    shared: Arc<Shared>,
    outbound: mpsc::Sender<Outbound>,
    max_message_bytes: usize,
) where
    R: AsyncBufRead + Unpin,
{
    loop {
        let message = match read_message(&mut reader, max_message_bytes).await {
            Ok(Some(message)) => message,
            Ok(None) => {
                shared
                    .terminate(LspError::Unavailable(
                        "language server closed its stdout".to_string(),
                    ))
                    .await;
                return;
            }
            Err(error) => {
                shared.terminate(error).await;
                return;
            }
        };

        if let Some(id) = response_id(&message) {
            let sender = shared.pending.lock().await.remove(&id);
            let Some(sender) = sender else {
                tracing::debug!(id, "ignoring response for unknown LSP request id");
                continue;
            };
            let result = if let Some(error) = message.get("error") {
                Err(parse_server_error(error))
            } else if let Some(result) = message.get("result") {
                Ok(result.clone())
            } else {
                Err(LspError::Protocol(format!(
                    "response for request {id} contains neither result nor error"
                )))
            };
            let _ = sender.send(result);
            continue;
        }

        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            if let Some(id) = message.get("id") {
                let _ = shared.events.send(LspEvent::Request {
                    id: id.clone(),
                    method: method.to_string(),
                    params,
                });
                // Until a caller-specific request handler is installed, keep
                // the JSON-RPC peer from waiting forever for a response.
                let _ = outbound
                    .send(Outbound::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("client method `{method}` is not supported"),
                        }
                    })))
                    .await;
            } else {
                let _ = shared.events.send(LspEvent::Notification {
                    method: method.to_string(),
                    params,
                });
            }
        }
    }
}

async fn supervise(
    mut child: Child,
    mut kill: oneshot::Receiver<()>,
    shared: Arc<Shared>,
    reader: tokio::task::JoinHandle<()>,
    writer: tokio::task::JoinHandle<()>,
    stderr: tokio::task::JoinHandle<String>,
) {
    let status = tokio::select! {
        status = child.wait() => status,
        _ = &mut kill => {
            let _ = child.kill().await;
            child.wait().await
        }
    };
    let stderr = stderr.await.unwrap_or_default();
    let error = match status {
        Ok(status) => LspError::ProcessExited {
            code: status.code(),
            stderr,
        },
        Err(error) => LspError::Io(error.to_string()),
    };
    shared.terminate(error).await;
    reader.abort();
    writer.abort();
}

async fn read_stderr(mut stderr: tokio::process::ChildStderr, max_bytes: usize) -> String {
    let mut output = Vec::new();
    let _ = stderr
        .read_to_end(&mut output)
        .await
        .map(|_| output.truncate(max_bytes));
    String::from_utf8_lossy(&output).into_owned()
}

fn encode_message(message: &Value) -> Result<Vec<u8>, LspError> {
    let payload = serde_json::to_vec(message)
        .map_err(|error| LspError::Protocol(format!("could not serialize request: {error}")))?;
    let mut frame = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
    frame.extend_from_slice(&payload);
    Ok(frame)
}

async fn read_message<R>(
    reader: &mut R,
    max_message_bytes: usize,
) -> Result<Option<Value>, LspError>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length = None;
    let mut header_bytes = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|error| LspError::Io(error.to_string()))?;
        if read == 0 {
            if content_length.is_none() && header_bytes == 0 {
                return Ok(None);
            }
            return Err(LspError::Protocol(
                "language server closed during message headers".to_string(),
            ));
        }
        header_bytes = header_bytes.saturating_add(read);
        if header_bytes > 16 * 1024 {
            return Err(LspError::Protocol(
                "language server headers exceed 16 KiB".to_string(),
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(LspError::Protocol(format!(
                "malformed header `{}`",
                line.trim()
            )));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            let length = value.trim().parse::<usize>().map_err(|_| {
                LspError::Protocol(format!("invalid Content-Length `{}`", value.trim()))
            })?;
            content_length = Some(length);
        }
    }

    let length = content_length.ok_or_else(|| {
        LspError::Protocol("language server message has no Content-Length header".to_string())
    })?;
    if length == 0 || length > max_message_bytes {
        return Err(LspError::Protocol(format!(
            "language server message length {length} is outside the allowed range 1..={max_message_bytes}"
        )));
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| LspError::Io(error.to_string()))?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(|error| LspError::Protocol(format!("invalid JSON payload: {error}")))
}

fn response_id(message: &Value) -> Option<u64> {
    message
        .get("id")
        .and_then(Value::as_u64)
        .filter(|_| message.get("method").is_none())
}

fn parse_server_error(error: &Value) -> LspError {
    LspError::Server {
        code: error.get("code").and_then(Value::as_i64).unwrap_or(-32000),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("language server request failed")
            .to_string(),
        data: error.get("data").cloned(),
    }
}

/// Operations understood by the small adapter below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspOperation {
    GoToDefinition,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    GoToImplementation,
    PrepareCallHierarchy,
    IncomingCalls,
    OutgoingCalls,
}

impl LspOperation {
    pub fn method(self) -> &'static str {
        match self {
            Self::GoToDefinition => "textDocument/definition",
            Self::FindReferences => "textDocument/references",
            Self::Hover => "textDocument/hover",
            Self::DocumentSymbol => "textDocument/documentSymbol",
            Self::WorkspaceSymbol => "workspace/symbol",
            Self::GoToImplementation => "textDocument/implementation",
            Self::PrepareCallHierarchy => "textDocument/prepareCallHierarchy",
            Self::IncomingCalls => "callHierarchy/incomingCalls",
            Self::OutgoingCalls => "callHierarchy/outgoingCalls",
        }
    }
}

/// A thin operation adapter that converts editor-facing paths and 1-based
/// positions into LSP URIs and 0-based positions. It is deliberately usable
/// by any runner; no server or tool crate is required.
#[derive(Debug, Clone)]
pub struct LspAdapter {
    client: LspClient,
    root: PathBuf,
    opened_documents: Arc<Mutex<HashMap<String, OpenedDocument>>>,
}

#[derive(Debug, Clone)]
struct OpenedDocument {
    version: i32,
    text: String,
}

impl LspAdapter {
    pub async fn start(
        config: LspServerConfig,
        root: impl Into<PathBuf>,
    ) -> Result<Self, LspError> {
        let root = root.into();
        let root_uri = config.root_uri.clone().or_else(|| file_uri(&root).ok());
        let initialization_options = config.initialization_options.clone().unwrap_or(Value::Null);
        let client = LspClient::spawn(config).await?;
        let initialize = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {},
            "initializationOptions": initialization_options,
            "workspaceFolders": root_uri.as_ref().map(|uri| vec![json!({
                "uri": uri,
                "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace"),
            })]),
        });
        if let Err(error) = client.initialize(initialize).await {
            let _ = client.shutdown().await;
            return Err(error);
        }
        Ok(Self {
            client,
            root,
            opened_documents: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn client(&self) -> &LspClient {
        &self.client
    }

    pub async fn request_method(&self, method: &str, params: Value) -> Result<Value, LspError> {
        self.client.request(method, params).await
    }

    /// Executes the position-based subset of the reference LSP tool.
    pub async fn request_operation(
        &self,
        operation: LspOperation,
        file: impl AsRef<Path>,
        line: usize,
        character: usize,
        query: Option<&str>,
    ) -> Result<Value, LspError> {
        let file = self.root.join(file.as_ref());
        let uri = file_uri(&file)?;
        self.synchronize_document(&file, &uri).await?;
        let position = json!({
            "line": line.checked_sub(1).ok_or_else(|| LspError::Protocol("line must be 1-based".to_string()))?,
            "character": character.checked_sub(1).ok_or_else(|| LspError::Protocol("character must be 1-based".to_string()))?,
        });
        if matches!(
            operation,
            LspOperation::IncomingCalls | LspOperation::OutgoingCalls
        ) {
            let prepare = self
                .request_method(
                    LspOperation::PrepareCallHierarchy.method(),
                    json!({
                        "textDocument": { "uri": uri },
                        "position": position,
                    }),
                )
                .await?;
            let item = prepare
                .as_array()
                .and_then(|items| items.first())
                .cloned()
                .filter(|item| item.is_object())
                .or_else(|| prepare.as_object().cloned().map(Value::Object))
                .ok_or_else(|| {
                    LspError::Protocol(format!(
                        "{} returned no call hierarchy item",
                        LspOperation::PrepareCallHierarchy.method()
                    ))
                })?;
            return self
                .request_method(operation.method(), json!({ "item": item }))
                .await;
        }

        let params = match operation {
            LspOperation::WorkspaceSymbol => json!({ "query": query.unwrap_or("") }),
            LspOperation::DocumentSymbol => json!({ "textDocument": { "uri": uri } }),
            LspOperation::FindReferences => json!({
                "textDocument": { "uri": uri },
                "position": position,
                "context": { "includeDeclaration": true },
            }),
            _ => json!({
                "textDocument": { "uri": uri },
                "position": position,
            }),
        };
        self.request_method(operation.method(), params).await
    }

    /// Keeps the server's in-memory document synchronized with the file being
    /// queried. OpenCode's LSP tool asks language servers about files that are
    /// not necessarily open in an editor, so the adapter must provide the
    /// didOpen/didChange lifecycle itself.
    async fn synchronize_document(&self, file: &Path, uri: &str) -> Result<(), LspError> {
        let text = tokio::fs::read_to_string(file).await.map_err(|error| {
            LspError::Io(format!(
                "could not read LSP document `{}`: {error}",
                file.display()
            ))
        })?;
        let mut opened = self.opened_documents.lock().await;
        let Some(previous) = opened.get(uri) else {
            self.client
                .notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": language_id(file),
                            "version": 1,
                            "text": text,
                        }
                    }),
                )
                .await?;
            opened.insert(uri.to_string(), OpenedDocument { version: 1, text });
            return Ok(());
        };
        if previous.text == text {
            return Ok(());
        }
        let version = previous.version.saturating_add(1);
        self.client
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            )
            .await?;
        opened.insert(uri.to_string(), OpenedDocument { version, text });
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), LspError> {
        self.client.shutdown().await
    }
}

fn file_uri(path: &Path) -> Result<String, LspError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| LspError::Io(error.to_string()))?
            .join(path)
    };
    url::Url::from_file_path(&absolute)
        .map(|url| url.to_string())
        .map_err(|_| {
            LspError::Protocol(format!("could not convert path to file URI: {absolute:?}"))
        })
}

fn language_id(path: &Path) -> &str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => "rust",
        Some("ts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js") => "javascript",
        Some("jsx") => "javascriptreact",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("c") => "c",
        Some("cpp") | Some("cc") | Some("cxx") => "cpp",
        Some("json") => "json",
        Some("toml") => "toml",
        Some("md") => "markdown",
        Some(extension) => extension,
        None => "plaintext",
    }
}
