//! Thread-owned production plugin manager.
//!
//! QuickJS runtimes are deliberately kept on one dedicated thread. The HTTP
//! server can therefore retain a cheap, `Send + Sync` handle without moving a
//! runtime between async worker threads. The manager wires local plugin
//! loading, event delivery, tool execution, and lifecycle disposal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::host::{
    LoadedPlugin, LoadedSummary, NoopHost, PluginBuilder, PluginHost, PluginToolCancellation,
};
use crate::js::transpile::transpile_module;
use crate::js::Runtime;
use crate::loader::ModuleResolver;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginLoadReport {
    pub spec: String,
    pub summary: Option<LoadedSummary>,
    pub error: Option<String>,
}

/// Typed, serializable request for a plugin prompt validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthValidateRequest {
    pub provider: String,
    pub method: usize,
    pub key: String,
    pub value: String,
}

/// Typed, serializable request for a plugin auth method's authorize function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthAuthorizeRequest {
    pub provider: String,
    pub method: usize,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
}

/// Typed, serializable request for a retained OAuth callback function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCallbackRequest {
    pub provider: String,
    pub method: usize,
    pub code: Option<String>,
}

enum Request {
    Load {
        spec: String,
        input: Value,
        options: Option<Value>,
        response: mpsc::Sender<PluginLoadReport>,
    },
    Tool {
        name: String,
        args: Value,
        context: Value,
        cancellation: Option<PluginToolCancellation>,
        timeout: Option<std::time::Duration>,
        response: mpsc::Sender<Result<Value, String>>,
    },
    ClientCancel {
        request_id: String,
        response: mpsc::Sender<Result<(), String>>,
    },
    AuthValidate {
        request: AuthValidateRequest,
        response: mpsc::Sender<Result<Option<String>, String>>,
    },
    AuthAuthorize {
        request: AuthAuthorizeRequest,
        response: mpsc::Sender<Result<Value, String>>,
    },
    AuthCallback {
        request: AuthCallbackRequest,
        response: mpsc::Sender<Result<Value, String>>,
    },
    Event {
        event: Value,
        response: mpsc::Sender<Result<(), String>>,
    },
    StreamEvent {
        event: Value,
    },
    Dispose {
        response: mpsc::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// The request queue capacity. The channel is bounded so a burst of stream
/// events cannot grow without bound; the stream-event path uses [`Request::try_send`]
/// and reports backpressure instead of blocking the server event fan-out.
const REQUEST_QUEUE_CAPACITY: usize = 1024;

/// A cloneable handle to the thread that owns all loaded QuickJS plugins.
pub struct PluginManager {
    sender: mpsc::SyncSender<Request>,
    join: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Clone for PluginManager {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            join: Arc::clone(&self.join),
        }
    }
}

impl PluginManager {
    /// Create a manager with a host that is safe to call from the plugin
    /// thread. The default host intentionally refuses unsupported effects.
    pub fn new() -> Self {
        Self::with_host(Arc::new(NoopHost))
    }

    pub fn with_host(host: Arc<dyn PluginHost>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(REQUEST_QUEUE_CAPACITY);
        let join = thread::Builder::new()
            .name("opencode-plugin-host".into())
            // QuickJS's interpreter and promise machinery use substantially
            // more native stack than the platform's small default worker
            // stack, even for synchronous plugin tools. Keep the runtime's
            // owner thread isolated while giving it enough headroom to avoid
            // turning a valid plugin call into a QuickJS stack-overflow.
            .stack_size(8 * 1024 * 1024)
            .spawn(move || worker(receiver, host))
            .expect("failed to spawn plugin host thread");
        Self {
            sender,
            join: Arc::new(Mutex::new(Some(join))),
        }
    }

    /// Load a local file plugin and return the registration summary. Supported
    /// specs are absolute paths, `file://` URLs, and paths relative to the
    /// current process directory.
    pub fn load_local(
        &self,
        spec: impl Into<String>,
        input: Value,
        options: Option<Value>,
    ) -> PluginLoadReport {
        let spec = spec.into();
        let (response, receiver) = mpsc::channel();
        if self
            .sender
            .send(Request::Load {
                spec: spec.clone(),
                input,
                options,
                response,
            })
            .is_err()
        {
            return PluginLoadReport {
                spec,
                summary: None,
                error: Some("plugin host thread is unavailable".into()),
            };
        }
        let report = receiver.recv().unwrap_or(PluginLoadReport {
            spec,
            summary: None,
            error: Some("plugin host thread stopped before loading".into()),
        });
        report
    }

    pub fn dispose(&self) -> Result<(), String> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(Request::Dispose { response })
            .map_err(|_| "plugin host thread is unavailable".to_string())?;
        receiver
            .recv()
            .unwrap_or_else(|_| Err("plugin host thread stopped before disposal".into()))
    }

    /// Execute a tool registered by one of the loaded plugins on the QuickJS
    /// owner thread.
    pub fn execute_tool(
        &self,
        name: impl Into<String>,
        args: Value,
        context: Value,
    ) -> Result<Value, String> {
        self.execute_tool_inner(name, args, context, None, None)
    }

    /// Execute a plugin tool while exposing `cancellation` to
    /// `context.abort.aborted`. The request remains on the QuickJS owner
    /// thread; the flag is the only cross-thread state shared with it.
    pub fn execute_tool_with_cancellation(
        &self,
        name: impl Into<String>,
        args: Value,
        context: Value,
        cancellation: PluginToolCancellation,
    ) -> Result<Value, String> {
        self.execute_tool_inner(name, args, context, Some(cancellation), None)
    }

    /// Execute a plugin tool with a wall-clock budget. Runaway plugin code is
    /// aborted by the QuickJS interrupt handler and reported as a limit error.
    pub fn execute_tool_with_timeout(
        &self,
        name: impl Into<String>,
        args: Value,
        context: Value,
        timeout: std::time::Duration,
    ) -> Result<Value, String> {
        self.execute_tool_inner(name, args, context, None, Some(timeout))
    }

    fn execute_tool_inner(
        &self,
        name: impl Into<String>,
        args: Value,
        context: Value,
        cancellation: Option<PluginToolCancellation>,
        timeout: Option<std::time::Duration>,
    ) -> Result<Value, String> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(Request::Tool {
                name: name.into(),
                args,
                context,
                cancellation,
                timeout,
                response,
            })
            .map_err(|_| "plugin host thread is unavailable".to_string())?;
        receiver
            .recv()
            .unwrap_or_else(|_| Err("plugin host thread stopped before tool execution".into()))
    }

    /// Deliver a server event to every loaded plugin that registered an event
    /// hook.
    pub fn event(&self, event: Value) -> Result<(), String> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(Request::Event { event, response })
            .map_err(|_| "plugin host thread is unavailable".to_string())?;
        receiver
            .recv()
            .unwrap_or_else(|_| Err("plugin host thread stopped before event delivery".into()))
    }

    /// Queue one event for the QuickJS-owned SSE handlers without blocking
    /// the server event fan-out on plugin callback execution.
    ///
    /// The queue is bounded: when the plugin owner thread is saturated, the
    /// event is dropped and a backpressure error is returned instead of
    /// growing the queue without bound.
    pub fn stream_event(&self, event: Value) -> Result<(), String> {
        self.sender
            .try_send(Request::StreamEvent { event })
            .map_err(|_| {
                "plugin SSE stream is at capacity; event dropped (backpressure)".to_string()
            })
    }

    /// Cancel an in-flight client request identified by its request id. The
    /// request id originates on the JS side (`__oc_client_call`) and is
    /// forwarded to the host's [`PluginHost::client_cancel`].
    pub fn client_cancel(&self, request_id: &str) -> Result<(), String> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(Request::ClientCancel {
                request_id: request_id.to_string(),
                response,
            })
            .map_err(|_| "plugin host thread is unavailable".to_string())?;
        receiver
            .recv()
            .unwrap_or_else(|_| Err("plugin host thread stopped before client cancellation".into()))
    }

    /// Run a plugin auth prompt validator on the QuickJS owner thread.
    pub fn auth_validate(&self, request: AuthValidateRequest) -> Result<Option<String>, String> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(Request::AuthValidate { request, response })
            .map_err(|_| "plugin host thread is unavailable".to_string())?;
        receiver
            .recv()
            .unwrap_or_else(|_| Err("plugin host thread stopped before auth validation".into()))
    }

    /// Run a plugin auth method's authorize function on the QuickJS owner
    /// thread. OAuth callbacks returned by the function remain owned by that
    /// same context until [`PluginManager::auth_callback`] is called.
    pub fn auth_authorize(&self, request: AuthAuthorizeRequest) -> Result<Value, String> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(Request::AuthAuthorize { request, response })
            .map_err(|_| "plugin host thread is unavailable".to_string())?;
        receiver
            .recv()
            .unwrap_or_else(|_| Err("plugin host thread stopped before auth authorization".into()))
    }

    /// Invoke the OAuth callback retained by a previous authorize request on
    /// the QuickJS owner thread.
    pub fn auth_callback(&self, request: AuthCallbackRequest) -> Result<Value, String> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(Request::AuthCallback { request, response })
            .map_err(|_| "plugin host thread is unavailable".to_string())?;
        receiver
            .recv()
            .unwrap_or_else(|_| Err("plugin host thread stopped before auth callback".into()))
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        if Arc::strong_count(&self.join) != 1 {
            return;
        }
        let _ = self.sender.send(Request::Shutdown);
        if let Ok(mut join) = self.join.lock() {
            if let Some(join) = join.take() {
                let _ = join.join();
            }
        }
    }
}

fn worker(receiver: mpsc::Receiver<Request>, host: Arc<dyn PluginHost>) {
    let mut plugins: Vec<LoadedPlugin> = Vec::new();
    while let Ok(request) = receiver.recv() {
        match request {
            Request::Load {
                spec,
                input,
                options,
                response,
            } => {
                let report = match Runtime::new() {
                    Ok(runtime) => load_one(&mut plugins, &host, &spec, input, options, runtime),
                    Err(error) => PluginLoadReport {
                        spec,
                        summary: None,
                        error: Some(format!("failed to create plugin runtime: {error}")),
                    },
                };
                let _ = response.send(report);
            }
            Request::Tool {
                name,
                args,
                context,
                cancellation,
                timeout,
                response,
            } => {
                let result = execute_tool(&plugins, &name, args, context, cancellation, timeout);
                let _ = response.send(result);
            }
            Request::ClientCancel {
                request_id,
                response,
            } => {
                host.client_cancel(&request_id);
                let _ = response.send(Ok(()));
            }
            Request::AuthValidate { request, response } => {
                let result = dispatch_auth_validate(&plugins, &request);
                let _ = response.send(result);
            }
            Request::AuthAuthorize { request, response } => {
                let result = dispatch_auth_authorize(&plugins, &request);
                let _ = response.send(result);
            }
            Request::AuthCallback { request, response } => {
                let result = dispatch_auth_callback(&plugins, &request);
                let _ = response.send(result);
            }
            Request::Event { event, response } => {
                let result = deliver_event(&plugins, event);
                let _ = response.send(result);
            }
            Request::StreamEvent { event } => {
                for plugin in &plugins {
                    if let Err(error) = plugin.stream_event(event.clone()) {
                        tracing::debug!(?error, "plugin SSE stream delivery failed");
                    }
                }
            }
            Request::Dispose { response } => {
                let result = dispose_all(&mut plugins);
                let _ = response.send(result);
            }
            Request::Shutdown => {
                let _ = dispose_all(&mut plugins);
                break;
            }
        }
    }
    let _ = dispose_all(&mut plugins);
}

fn load_one(
    plugins: &mut Vec<LoadedPlugin>,
    host: &Arc<dyn PluginHost>,
    spec: &str,
    input: Value,
    options: Option<Value>,
    runtime: Runtime,
) -> PluginLoadReport {
    let path = match local_path(spec) {
        Ok(path) => path,
        Err(error) => {
            return PluginLoadReport {
                spec: spec.to_string(),
                summary: None,
                error: Some(error),
            }
        }
    };
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            return PluginLoadReport {
                spec: spec.to_string(),
                summary: None,
                error: Some(format!("failed to read {}: {error}", path.display())),
            }
        }
    };
    let code = match transpile_module(&source) {
        Ok(code) => code,
        Err(error) => {
            return PluginLoadReport {
                spec: spec.to_string(),
                summary: None,
                error: Some(format!("failed to transpile {}: {error}", path.display())),
            }
        }
    };
    let resolver = Arc::new(ModuleResolver::new(
        path.parent().unwrap_or_else(|| Path::new(".")),
    ));
    let mut plugin =
        match PluginBuilder::new(Arc::clone(host), resolver).build_with_runtime(runtime) {
            Ok(plugin) => plugin,
            Err(error) => {
                return PluginLoadReport {
                    spec: spec.to_string(),
                    summary: None,
                    error: Some(format!("failed to create plugin runtime: {error}")),
                }
            }
        };
    if let Err(error) = plugin.load(&code, &path.to_string_lossy(), input, options) {
        return PluginLoadReport {
            spec: spec.to_string(),
            summary: None,
            error: Some(format!("failed to load {}: {error}", path.display())),
        };
    }
    let summary = plugin.summary().clone();
    plugins.push(plugin);
    PluginLoadReport {
        spec: spec.to_string(),
        summary: Some(summary),
        error: None,
    }
}

fn execute_tool(
    plugins: &[LoadedPlugin],
    name: &str,
    args: Value,
    context: Value,
    cancellation: Option<PluginToolCancellation>,
    timeout: Option<std::time::Duration>,
) -> Result<Value, String> {
    let plugin = plugins
        .iter()
        .find(|plugin| plugin.tool_names().iter().any(|tool| tool == name))
        .ok_or_else(|| format!("plugin tool '{name}' is not registered"))?;
    match timeout {
        Some(timeout) => plugin
            .execute_tool_with_timeout(name, args, context, timeout)
            .map_err(|error| error.to_string()),
        None => plugin
            .execute_tool_with_cancellation(name, args, context, cancellation)
            .map_err(|error| error.to_string()),
    }
}

fn deliver_event(plugins: &[LoadedPlugin], event: Value) -> Result<(), String> {
    for plugin in plugins.iter().filter(|plugin| plugin.has_hook("event")) {
        plugin
            .event(event.clone())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn auth_plugin<'a>(
    plugins: &'a [LoadedPlugin],
    provider: &str,
) -> Result<&'a LoadedPlugin, String> {
    plugins
        .iter()
        .find(|plugin| {
            plugin
                .summary()
                .auth
                .iter()
                .any(|auth| auth.provider == provider)
        })
        .ok_or_else(|| format!("plugin auth provider '{provider}' is not registered"))
}

fn dispatch_auth_validate(
    plugins: &[LoadedPlugin],
    request: &AuthValidateRequest,
) -> Result<Option<String>, String> {
    auth_plugin(plugins, &request.provider).and_then(|plugin| {
        plugin
            .auth_validate(
                &request.provider,
                request.method,
                &request.key,
                &request.value,
            )
            .map_err(|error| error.to_string())
    })
}

fn dispatch_auth_authorize(
    plugins: &[LoadedPlugin],
    request: &AuthAuthorizeRequest,
) -> Result<Value, String> {
    auth_plugin(plugins, &request.provider).and_then(|plugin| {
        plugin
            .auth_authorize(&request.provider, request.method, &request.inputs)
            .map_err(|error| error.to_string())
    })
}

fn dispatch_auth_callback(
    plugins: &[LoadedPlugin],
    request: &AuthCallbackRequest,
) -> Result<Value, String> {
    auth_plugin(plugins, &request.provider).and_then(|plugin| {
        plugin
            .auth_callback(&request.provider, request.method, request.code.as_deref())
            .map_err(|error| error.to_string())
    })
}

fn dispose_all(plugins: &mut Vec<LoadedPlugin>) -> Result<(), String> {
    let mut first_error = None;
    for plugin in plugins.iter() {
        if let Err(error) = plugin.dispose() {
            if first_error.is_none() {
                first_error = Some(error.to_string());
            }
        }
    }
    plugins.clear();
    first_error.map_or(Ok(()), Err)
}

fn local_path(spec: &str) -> Result<PathBuf, String> {
    let raw = spec.strip_prefix("file://").unwrap_or(spec);
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| format!("failed to resolve plugin directory: {error}"))?
            .join(path)
    };
    std::fs::canonicalize(&path)
        .map_err(|error| format!("plugin path {} is unavailable: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::PluginManager;

    #[test]
    fn missing_plugin_returns_report_without_panicking() {
        let manager = PluginManager::new();
        let report = manager.load_local(
            "file:///definitely/missing/opencode-plugin.ts",
            serde_json::json!({}),
            None,
        );
        assert!(report.summary.is_none());
        assert!(report.error.unwrap().contains("unavailable"));
        manager.dispose().unwrap();
    }

    #[test]
    fn dispose_is_idempotent_for_empty_manager() {
        let manager = PluginManager::new();
        manager.dispose().unwrap();
        manager.dispose().unwrap();
    }

    #[test]
    fn loads_a_local_plugin_on_the_host_thread() {
        let manager = PluginManager::new();
        let spec = format!(
            "file://{}/tests/fixtures/example.ts",
            env!("CARGO_MANIFEST_DIR")
        );
        let report = manager.load_local(spec, serde_json::json!({}), None);
        let summary = report
            .summary
            .expect("fixture plugin should load through the manager");
        assert_eq!(summary.tools.len(), 1);
        assert_eq!(summary.tools[0].name, "mytool");
        manager.dispose().unwrap();
    }

    #[test]
    fn executes_a_registered_local_plugin_tool_on_the_host_thread() {
        let manager = PluginManager::new();
        let spec = format!(
            "file://{}/tests/fixtures/example.ts",
            env!("CARGO_MANIFEST_DIR")
        );
        let report = manager.load_local(spec, serde_json::json!({}), None);
        assert!(report.error.is_none());
        let result = manager
            .execute_tool(
                "mytool",
                serde_json::json!({ "foo": "world" }),
                serde_json::json!({ "callID": "call-1" }),
            )
            .expect("registered plugin tool should execute");
        assert_eq!(result, serde_json::json!("Hello world!"));
        manager.dispose().unwrap();
    }

    #[test]
    fn executes_a_synchronous_local_plugin_tool_on_the_host_thread() {
        let manager = PluginManager::new();
        let spec = format!(
            "file://{}/tests/fixtures/sync-tool.ts",
            env!("CARGO_MANIFEST_DIR")
        );
        let report = manager.load_local(spec, serde_json::json!({}), None);
        assert!(report.error.is_none());
        let result = manager
            .execute_tool(
                "synctool",
                serde_json::json!({ "foo": "world" }),
                serde_json::json!({ "callID": "call-1" }),
            )
            .expect("registered plugin tool should execute");
        assert_eq!(result, serde_json::json!("Sync world!"));
        manager.dispose().unwrap();
    }
}
