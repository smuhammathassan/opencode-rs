//! Plugin host: the Rust-side services plugins reach through the JS bridge,
//! plus the manager that loads a JS plugin and triggers its hooks.
//!
//! Mirrors the reference `Plugin` service in
//! reference/packages/opencode/src/plugin/index.ts (trigger/list/init) and the
//! host-side services behind `PluginInput` in reference/packages/plugin/src/index.ts.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::js::{JsError, JsValue, Runtime};
use crate::registration::{ClientRpcRequest, PluginRegistrationSink};

/// Cooperative cancellation state for one plugin tool invocation.
///
/// The flag is intentionally separate from the async runtime: the server can
/// set it from its Tokio task while the QuickJS owner thread continues to
/// pump promise jobs. Plugin code observes it through `context.abort`.
#[derive(Clone, Debug, Default)]
pub struct PluginToolCancellation {
    cancelled: Arc<AtomicBool>,
}

impl PluginToolCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Host-side services the JS bridge can reach.
///
/// The reference provides these to plugins via `PluginInput` (a client, a
/// shell, fs access, ...) by running plugins inside the opencode process. Here
/// the integrating application implements this trait; every method has a
/// default that fails or no-ops so a minimal host can be stood up quickly.
///
/// TODO(integration): wire the real oc-server client / oc-tool shell behind
/// these methods once those crates land.
#[allow(unused_variables)]
pub trait PluginHost: Send + Sync {
    /// OpenCode client RPC (`client.<group>.<method>`). Mirrors the methods of
    /// the `@opencode-ai/sdk` client exposed as `PluginInput.client`.
    fn client(&self, method: &str, args: &Value) -> Result<Value, String> {
        Err(format!("client.{method} is not implemented by the host"))
    }
    /// Typed adapter for plugin client calls. Existing hosts can keep
    /// implementing [`PluginHost::client`]; structured RPC routers can
    /// override this method instead.
    fn client_rpc(&self, request: &ClientRpcRequest) -> Result<Value, String> {
        self.client(&request.method, &request.args)
    }
    /// HTTP fetch used by the `fetch` polyfill.
    fn fetch(
        &self,
        url: &str,
        method: &str,
        headers: &Value,
        body: Option<&str>,
    ) -> Result<Value, String> {
        Err("fetch is not implemented by the host".into())
    }
    /// Shell execution used by the `$` shim. Returns `{ stdout, stderr, exitCode }`.
    fn shell_exec(&self, request: &Value) -> Result<Value, String> {
        Err("shell is not implemented by the host".into())
    }
    /// Brace expansion used by `$.braces`.
    fn shell_braces(&self, pattern: &str) -> Result<Vec<String>, String> {
        Ok(vec![pattern.to_string()])
    }
    /// Filesystem operations for `node:fs/promises`.
    fn fs(&self, method: &str, args: &Value) -> Result<Value, String> {
        Err(format!("fs.{method} is not implemented by the host"))
    }
    /// OS facts for `node:os` / `node:process`.
    fn os(&self, name: &str) -> Option<String> {
        None
    }
    /// Plugin log output.
    fn log(&self, level: &str, message: &str) {}
    /// A v1 workspace adapter was registered via `experimental_workspace`.
    fn workspace_adapter_registered(&self, type_: &str) {}
    /// A v1 declarative registration (agent/command/skill/provider/...) arrived.
    fn register_v1(&self, kind: &str, input: &Value) {}
    /// Optional typed sink for registrations discovered by the plugin runtime.
    fn registration_sink(&self) -> Option<Arc<dyn PluginRegistrationSink>> {
        None
    }
    /// A v2 domain transform callback was registered by a plugin.
    fn v2_transform_registered(&self, domain: &str) {}
    /// A v2 domain reload was requested.
    fn v2_reload(&self, domain: &str) {}
    /// A tool asked for permission via its `context.ask(...)`.
    fn tool_ask(&self, _call_id: &str, _input: &Value) -> Result<Value, String> {
        Ok(serde_json::json!({ "status": "allow" }))
    }
}

/// The default no-op host. Useful for tests and minimal embeddings.
pub struct NoopHost;

impl PluginHost for NoopHost {}

/// A local host for production plugin bootstrap. It implements the effects
/// that are independent of the server's HTTP/client state while leaving the
/// client RPC surface injectable through [`PluginHost`].
#[derive(Default)]
pub struct LocalHost {
    registration_sink: Option<Arc<dyn PluginRegistrationSink>>,
    client_rpc_handler:
        Option<Arc<dyn Fn(&ClientRpcRequest) -> Result<Value, String> + Send + Sync>>,
}

impl LocalHost {
    /// Build a local host that forwards declarative plugin registrations to
    /// the embedding application's runtime registry.
    pub fn with_registration_sink(sink: Arc<dyn PluginRegistrationSink>) -> Self {
        Self {
            registration_sink: Some(sink),
            ..Self::default()
        }
    }

    /// Install a synchronous client-RPC adapter supplied by the embedding
    /// application. The callback must be a read-only, non-blocking snapshot
    /// operation: QuickJS invokes it on the plugin owner thread.
    pub fn with_client_rpc(
        mut self,
        handler: impl Fn(&ClientRpcRequest) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Self {
        self.client_rpc_handler = Some(Arc::new(handler));
        self
    }
}

impl PluginHost for LocalHost {
    fn client_rpc(&self, request: &ClientRpcRequest) -> Result<Value, String> {
        if let Some(handler) = &self.client_rpc_handler {
            return handler(request);
        }
        self.client(&request.method, &request.args)
    }

    fn registration_sink(&self) -> Option<Arc<dyn PluginRegistrationSink>> {
        self.registration_sink.clone()
    }

    fn fetch(
        &self,
        url: &str,
        method: &str,
        headers: &Value,
        body: Option<&str>,
    ) -> Result<Value, String> {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|error| format!("invalid fetch method: {error}"))?;
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| format!("fetch client: {error}"))?;
        let mut request = client.request(method, url);
        if let Some(values) = headers.as_object() {
            for (name, value) in values {
                if let Some(value) = value.as_str() {
                    request = request.header(name, value);
                }
            }
        }
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        let response = request.send().map_err(|error| format!("fetch: {error}"))?;
        let status = response.status().as_u16();
        let url = response.url().to_string();
        let response_headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((
                    name.to_string(),
                    Value::String(value.to_str().ok()?.to_string()),
                ))
            })
            .collect::<serde_json::Map<_, _>>();
        let body = response
            .bytes()
            .map_err(|error| format!("fetch body: {error}"))?;
        Ok(serde_json::json!({
            "status": status,
            "headers": response_headers,
            "body": String::from_utf8_lossy(&body),
            "url": url,
        }))
    }

    fn shell_exec(&self, request: &Value) -> Result<Value, String> {
        let command = request
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| "shell command is required".to_string())?;
        let mut process = if cfg!(windows) {
            let mut process = Command::new("cmd");
            process.args(["/C", command]);
            process
        } else {
            let mut process = Command::new("sh");
            process.args(["-c", command]);
            process
        };
        if let Some(cwd) = request.get("cwd").and_then(Value::as_str) {
            if !cwd.is_empty() {
                process.current_dir(cwd);
            }
        }
        if let Some(env) = request.get("env").and_then(Value::as_object) {
            for (key, value) in env {
                if let Some(value) = value.as_str() {
                    process.env(key, value);
                }
            }
        }
        let output = process
            .output()
            .map_err(|error| format!("shell: {error}"))?;
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let nothrow = request
            .get("nothrow")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !nothrow && exit_code != 0 {
            return Err(format!("shell exited with code {exit_code}: {stderr}"));
        }
        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exitCode": exit_code,
        }))
    }

    fn shell_braces(&self, pattern: &str) -> Result<Vec<String>, String> {
        let Some(start) = pattern.find('{') else {
            return Ok(vec![pattern.to_string()]);
        };
        let Some(relative_end) = pattern[start + 1..].find('}') else {
            return Ok(vec![pattern.to_string()]);
        };
        let end = start + 1 + relative_end;
        let alternatives = pattern[start + 1..end]
            .split(',')
            .map(str::to_string)
            .collect::<Vec<_>>();
        if alternatives.len() < 2 {
            return Ok(vec![pattern.to_string()]);
        }
        Ok(alternatives
            .into_iter()
            .map(|alternative| {
                format!(
                    "{}{}{}",
                    &pattern[..start],
                    alternative,
                    &pattern[end + 1..]
                )
            })
            .collect())
    }

    fn fs(&self, method: &str, args: &Value) -> Result<Value, String> {
        let args = args
            .as_array()
            .ok_or_else(|| "filesystem arguments must be an array".to_string())?;
        let path = |index: usize| {
            args.get(index)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("fs.{method} requires a path"))
        };
        let success = |data: Value| serde_json::json!({ "ok": true, "data": data });
        let failure = |error: std::io::Error| {
            let code = match error.kind() {
                std::io::ErrorKind::NotFound => "ENOENT",
                std::io::ErrorKind::PermissionDenied => "EACCES",
                std::io::ErrorKind::AlreadyExists => "EEXIST",
                _ => "EIO",
            };
            serde_json::json!({ "ok": false, "error": error.to_string(), "code": code })
        };
        match method {
            "mkdir" => {
                let path = path(0)?;
                let recursive = args
                    .get(1)
                    .and_then(Value::as_object)
                    .and_then(|options| options.get("recursive"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let result = if recursive {
                    std::fs::create_dir_all(path)
                } else {
                    std::fs::create_dir(path)
                };
                Ok(match result {
                    Ok(()) => success(Value::Null),
                    Err(error) => failure(error),
                })
            }
            "rm" => {
                let path = path(0)?;
                let options = args.get(1).and_then(Value::as_object);
                let recursive = options
                    .and_then(|options| options.get("recursive"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let force = options
                    .and_then(|options| options.get("force"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let result = if recursive {
                    std::fs::remove_dir_all(path)
                } else if std::path::Path::new(path).is_dir() {
                    std::fs::remove_dir(path)
                } else {
                    std::fs::remove_file(path)
                };
                Ok(match result {
                    Ok(()) => success(Value::Null),
                    Err(error) if force && error.kind() == std::io::ErrorKind::NotFound => {
                        success(Value::Null)
                    }
                    Err(error) => failure(error),
                })
            }
            "readFile" => match std::fs::read(path(0)?) {
                Ok(bytes) => Ok(success(Value::String(
                    String::from_utf8_lossy(&bytes).into_owned(),
                ))),
                Err(error) => Ok(failure(error)),
            },
            "writeFile" => {
                let path = path(0)?;
                let data = args.get(1).cloned().unwrap_or(Value::Null);
                let bytes = match data {
                    Value::String(value) => value.into_bytes(),
                    value => serde_json::to_vec(&value).map_err(|error| error.to_string())?,
                };
                Ok(match std::fs::write(path, bytes) {
                    Ok(()) => success(Value::Null),
                    Err(error) => failure(error),
                })
            }
            "readdir" => match std::fs::read_dir(path(0)?) {
                Ok(entries) => {
                    let names = entries
                        .filter_map(Result::ok)
                        .filter_map(|entry| entry.file_name().into_string().ok())
                        .map(Value::String)
                        .collect::<Vec<_>>();
                    Ok(success(Value::Array(names)))
                }
                Err(error) => Ok(failure(error)),
            },
            "stat" => match std::fs::metadata(path(0)?) {
                Ok(metadata) => Ok(success(serde_json::json!({
                    "size": metadata.len(),
                    "isFile": metadata.is_file(),
                    "isDirectory": metadata.is_dir(),
                }))),
                Err(error) => Ok(failure(error)),
            },
            "access" => Ok(match std::fs::metadata(path(0)?) {
                Ok(_) => success(Value::Null),
                Err(error) => failure(error),
            }),
            "readlink" => match std::fs::read_link(path(0)?) {
                Ok(target) => Ok(success(Value::String(
                    target.to_string_lossy().into_owned(),
                ))),
                Err(error) => Ok(failure(error)),
            },
            "readJson" => match std::fs::read_to_string(path(0)?) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(value) => Ok(success(value)),
                    Err(error) => Ok(serde_json::json!({
                        "ok": false,
                        "error": error.to_string(),
                        "code": "EJSONPARSE"
                    })),
                },
                Err(error) => Ok(failure(error)),
            },
            "writeJson" => {
                let path = path(0)?;
                let value = args.get(1).cloned().unwrap_or(Value::Null);
                let contents =
                    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
                Ok(match std::fs::write(path, format!("{contents}\n")) {
                    Ok(()) => success(Value::Null),
                    Err(error) => failure(error),
                })
            }
            "exists" => Ok(success(Value::Bool(
                std::path::Path::new(path(0)?).exists(),
            ))),
            "realpath" => match std::fs::canonicalize(path(0)?) {
                Ok(path) => Ok(success(Value::String(path.to_string_lossy().into_owned()))),
                Err(error) => Ok(failure(error)),
            },
            _ => Ok(serde_json::json!({
                "ok": false,
                "error": format!("fs.{method} is not implemented by the local host"),
                "code": "ENOSYS"
            })),
        }
    }

    fn os(&self, name: &str) -> Option<String> {
        match name {
            "homedir" => std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok(),
            "tmpdir" => Some(std::env::temp_dir().to_string_lossy().into_owned()),
            "platform" => Some(std::env::consts::OS.to_string()),
            "cwd" => std::env::current_dir()
                .ok()
                .map(|path| path.to_string_lossy().into_owned()),
            _ => None,
        }
    }

    fn log(&self, level: &str, message: &str) {
        match level {
            "error" => tracing::error!(target: "opencode.plugin", "{message}"),
            "warn" => tracing::warn!(target: "opencode.plugin", "{message}"),
            "debug" => tracing::debug!(target: "opencode.plugin", "{message}"),
            _ => tracing::info!(target: "opencode.plugin", "{message}"),
        }
    }
}

#[cfg(test)]
mod local_host_tests {
    use super::{LocalHost, PluginHost};
    use serde_json::json;

    #[test]
    fn local_host_expands_braces_and_runs_shell() {
        let host = LocalHost::default();
        assert_eq!(
            host.shell_braces("src/{a,b}.rs").unwrap(),
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
        );
        let result = host
            .shell_exec(&json!({ "command": "printf hello" }))
            .unwrap();
        assert_eq!(result["stdout"], "hello");
        assert_eq!(result["exitCode"], 0);
    }

    #[test]
    fn local_host_reads_and_writes_json_files() {
        let host = LocalHost::default();
        let root =
            std::env::temp_dir().join(format!("oc-plugin-local-host-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("value.json");
        let path = path.to_string_lossy().into_owned();
        let result = host
            .fs("writeJson", &json!([path.clone(), { "answer": 42 }]))
            .unwrap();
        assert_eq!(result["ok"], true);
        let result = host.fs("readJson", &json!([path])).unwrap();
        assert_eq!(result["data"]["answer"], 42);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_host_dispatches_client_rpc_through_embedding_callback() {
        let host = LocalHost::default().with_client_rpc(|request| {
            assert_eq!(request.method, "session.status");
            assert_eq!(request.args, json!(null));
            Ok(json!({ "data": { "ses_1": { "status": "idle" } } }))
        });
        let result = host
            .client_rpc(&crate::registration::ClientRpcRequest::new(
                "session.status",
                json!(null),
            ))
            .unwrap();
        assert_eq!(result["data"]["ses_1"]["status"], "idle");
    }
}

/// Result of loading a plugin module: which hooks it exposes and which tools it
/// registered.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LoadedSummary {
    #[serde(rename = "hookNames")]
    pub hook_names: Vec<String>,
    pub tools: Vec<ToolInfo>,
    #[serde(rename = "pluginId", default)]
    pub plugin_id: Option<String>,
    /// Serializable metadata for executable plugin auth hooks. Function
    /// values stay inside QuickJS and are addressed by manager requests.
    #[serde(default)]
    pub auth: Vec<PluginAuthSummary>,
}

/// A serializable summary of one `hooks.auth` registration.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PluginAuthSummary {
    pub provider: String,
    pub methods: Vec<PluginAuthMethodSummary>,
}

/// A serializable auth method descriptor. The executable `authorize` function
/// is deliberately not represented here.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PluginAuthMethodSummary {
    #[serde(rename = "type")]
    pub r#type: PluginAuthMethodType,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<PluginAuthPromptSummary>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginAuthMethodType {
    OAuth,
    Api,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginAuthPromptSummary {
    Text {
        key: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<PluginAuthWhenSummary>,
    },
    Select {
        key: String,
        message: String,
        options: Vec<PluginAuthOptionSummary>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<PluginAuthWhenSummary>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PluginAuthOptionSummary {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PluginAuthWhenSummary {
    pub key: String,
    pub op: String,
    pub value: String,
}

/// A tool definition registered by a plugin, with its JSON schema.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// A loaded JS plugin bound to its own QuickJS context.
///
/// `Runtime` and `ModuleResolver` are not `Sync`, so a plugin must be used from
/// a single thread; the integrating application should keep plugin execution on
/// one thread (the reference runs plugin hooks in the event loop).
pub struct LoadedPlugin {
    runtime: Runtime,
    summary: LoadedSummary,
    active_cancellation: Arc<Mutex<Option<PluginToolCancellation>>>,
}

impl LoadedPlugin {
    /// Run a (input, output) trigger hook. Mirrors the reference
    /// `Plugin.trigger(name, input, output)`; hooks may mutate `output` and it
    /// is returned.
    pub fn trigger(&self, name: &str, input: Value, output: Value) -> Result<Value, JsError> {
        let payload = serde_json::json!({ "name": name, "input": input, "output": output });
        self.async_call("__oc_trigger", payload)
    }

    /// Call the `config` hook with the current config.
    pub fn config(&self, config: Value) -> Result<(), JsError> {
        let payload = serde_json::json!({ "config": config });
        self.async_call("__oc_config", payload).map(|_| ())
    }

    /// Deliver an event to the `event` hook.
    pub fn event(&self, event: Value) -> Result<(), JsError> {
        let payload = serde_json::json!({ "event": event });
        self.async_call("__oc_event", payload).map(|_| ())
    }

    /// Deliver one serialized server event to the QuickJS-owned SSE handlers.
    /// The handlers never cross the FFI boundary; `__oc_stream_emit` pumps
    /// their Promise reactions on this plugin's owner thread.
    pub fn stream_event(&self, event: Value) -> Result<(), JsError> {
        self.async_call("__oc_stream_emit", event).map(|_| ())
    }

    /// Call the `dispose` hook on every registered hook set.
    pub fn dispose(&self) -> Result<(), JsError> {
        self.async_call("__oc_dispose", serde_json::Value::Null)
            .map(|_| ())
    }

    /// Execute a plugin tool by name. `context` mirrors the reference
    /// `ToolContext`.
    pub fn execute_tool(&self, name: &str, args: Value, context: Value) -> Result<Value, JsError> {
        self.execute_tool_with_cancellation(name, args, context, None)
    }

    /// Execute a tool with an optional cooperative cancellation flag. The
    /// flag is visible to the tool's `context.abort.aborted` getter while the
    /// owner thread pumps async promise jobs.
    pub fn execute_tool_with_cancellation(
        &self,
        name: &str,
        args: Value,
        context: Value,
        cancellation: Option<PluginToolCancellation>,
    ) -> Result<Value, JsError> {
        let payload = serde_json::json!({ "name": name, "args": args, "context": context });
        {
            let mut active = self
                .active_cancellation
                .lock()
                .map_err(|_| JsError::Internal("plugin cancellation state is poisoned".into()))?;
            *active = cancellation;
        }
        let result = self.async_call("__oc_tool_execute", payload);
        if let Ok(mut active) = self.active_cancellation.lock() {
            *active = None;
        }
        result
    }

    /// Invoke a workspace adapter method (`configure`/`create`/`remove`/`target`).
    pub fn workspace_adapter(
        &self,
        type_: &str,
        method: &str,
        args: Value,
    ) -> Result<Value, JsError> {
        let payload = serde_json::json!({ "type": type_, "method": method, "args": args });
        self.async_call("__oc_workspace_adapter", payload)
    }

    /// Invoke a v2 domain transform callback with a mutable draft. Returns the
    /// (possibly mutated) draft.
    pub fn v2_transform(&self, domain: &str, draft: Value) -> Result<Value, JsError> {
        let payload = serde_json::json!({ "domain": domain, "draft": draft });
        self.async_call("__oc_v2_transform", payload)
    }

    /// The hooks and tools this plugin registered.
    pub fn summary(&self) -> &LoadedSummary {
        &self.summary
    }

    /// The plugin id (the module's `id` export for npm plugins, or `null`).
    pub fn plugin_id(&self) -> Option<&str> {
        self.summary.plugin_id.as_deref()
    }

    /// A list of the tool names this plugin registered.
    pub fn tool_names(&self) -> Vec<String> {
        self.summary
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect()
    }

    /// Does this plugin register any (input, output) trigger hooks?
    pub fn has_hook(&self, name: &str) -> bool {
        self.summary.hook_names.iter().any(|n| n == name)
    }

    /// Invoke a prompt validator inside this plugin's QuickJS owner context.
    /// A missing validator is represented as `Ok(None)`.
    pub fn auth_validate(
        &self,
        provider: &str,
        method: usize,
        key: &str,
        value: &str,
    ) -> Result<Option<String>, JsError> {
        let payload = serde_json::json!({
            "provider": provider,
            "method": method,
            "key": key,
            "value": value,
        });
        let result = self.async_call("__oc_auth_validate", payload)?;
        Ok(match result {
            Value::Null => None,
            Value::String(message) => Some(message),
            other => Some(other.to_string()),
        })
    }

    /// Start an auth method inside QuickJS. For OAuth methods the returned
    /// JSON is the authorization descriptor; the function-valued callback is
    /// retained by QuickJS for the subsequent `auth_callback` call. API
    /// methods may return their own serializable success/failed result.
    pub fn auth_authorize(
        &self,
        provider: &str,
        method: usize,
        inputs: &std::collections::BTreeMap<String, String>,
    ) -> Result<Value, JsError> {
        let payload = serde_json::json!({
            "provider": provider,
            "method": method,
            "inputs": inputs,
        });
        self.async_call("__oc_auth_authorize", payload)
    }

    /// Complete the callback retained by the preceding OAuth authorize call.
    /// This stays on the QuickJS owner thread and never serializes the
    /// callback function itself.
    pub fn auth_callback(
        &self,
        provider: &str,
        method: usize,
        code: Option<&str>,
    ) -> Result<Value, JsError> {
        let payload = serde_json::json!({
            "provider": provider,
            "method": method,
            "code": code,
        });
        self.async_call("__oc_auth_callback", payload)
    }

    fn async_call(&self, name: &str, payload: Value) -> Result<Value, JsError> {
        let payload = json_string(&payload)?;
        self.runtime.set_global_null("__oc_pending")?;
        let active_cancellation = Arc::clone(&self.active_cancellation);
        self.runtime.call_function_and_pump_with_probe(
            "__oc_async_call",
            vec![JsValue::from(name), JsValue::from(payload)],
            || {
                let cancelled = active_cancellation
                    .lock()
                    .map(|active| {
                        active
                            .as_ref()
                            .is_some_and(PluginToolCancellation::is_cancelled)
                    })
                    .unwrap_or(true);
                if cancelled {
                    let _ = self
                        .runtime
                        .call_function("__oc_tool_abort_notify", Vec::<JsValue>::new())?;
                }
                Ok(())
            },
        )?;
        let pending = self.runtime.global("__oc_pending")?;
        // The JS side writes `__oc_pending` as a JSON-encoded string; decode it.
        let result: Value = match pending {
            Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
            other => other,
        };
        match result.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(result.get("value").cloned().unwrap_or(Value::Null)),
            _ => {
                let message = result
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("plugin call failed: {result}"));
                Err(JsError::Exception(message))
            }
        }
    }
}

fn json_string(value: &Value) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|e| JsError::Internal(e.to_string()))
}

/// A builder for [`LoadedPlugin`].
pub struct PluginBuilder {
    host: Arc<dyn PluginHost>,
    resolver: Arc<crate::loader::ModuleResolver>,
}

impl PluginBuilder {
    pub fn new(host: Arc<dyn PluginHost>, resolver: Arc<crate::loader::ModuleResolver>) -> Self {
        Self { host, resolver }
    }

    /// Create the QuickJS context and install the polyfill runtime.
    pub fn build(&self) -> Result<LoadedPlugin, JsError> {
        self.build_with_runtime(Runtime::new()?)
    }

    /// Finish building a plugin around a runtime created by its owner loop.
    /// QuickJS records its native stack baseline during `Runtime::new`; the
    /// manager uses this entrypoint so that baseline is not captured inside a
    /// short-lived loader frame and then reused from a shallower request-loop
    /// frame.
    pub fn build_with_runtime(&self, runtime: Runtime) -> Result<LoadedPlugin, JsError> {
        runtime.install_bridge(self.host.clone(), self.resolver.clone())?;
        let active_cancellation = Arc::new(Mutex::new(None));
        let cancellation_for_callback = Arc::clone(&active_cancellation);
        runtime.add_callback("__oc_tool_cancelled", move || {
            cancellation_for_callback
                .lock()
                .map(|active| {
                    active
                        .as_ref()
                        .is_some_and(PluginToolCancellation::is_cancelled)
                })
                .unwrap_or(true)
        })?;
        runtime.eval(
            crate::polyfill::RUNTIME_SOURCE,
            "opencode-polyfill-runtime.js",
        )?;
        Ok(LoadedPlugin {
            runtime,
            summary: LoadedSummary {
                hook_names: Vec::new(),
                tools: Vec::new(),
                plugin_id: None,
                auth: Vec::new(),
            },
            active_cancellation,
        })
    }
}

impl LoadedPlugin {
    /// Evaluate the plugin's main entry module and run its plugin function.
    pub fn load(
        &mut self,
        code: &str,
        filename: &str,
        input: Value,
        options: Option<Value>,
    ) -> Result<(), JsError> {
        self.runtime.call_function(
            "__oc_eval_main",
            vec![JsValue::from(code), JsValue::from(filename)],
        )?;
        let payload = serde_json::json!({ "input": input, "options": options });
        let summary = self.async_call("__oc_load_plugin", payload)?;
        self.summary = serde_json::from_value(summary)
            .map_err(|e| JsError::Internal(format!("invalid plugin summary: {e}")))?;
        Ok(())
    }
}
