//! Plugin host: the Rust-side services plugins reach through the JS bridge,
//! plus the manager that loads a JS plugin and triggers its hooks.
//!
//! Mirrors the reference `Plugin` service in
//! reference/packages/opencode/src/plugin/index.ts (trigger/list/init) and the
//! host-side services behind `PluginInput` in reference/packages/plugin/src/index.ts.

use std::sync::Arc;

use serde_json::Value;

use crate::js::{JsError, JsValue, Runtime};

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

/// Result of loading a plugin module: which hooks it exposes and which tools it
/// registered.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LoadedSummary {
    #[serde(rename = "hookNames")]
    pub hook_names: Vec<String>,
    pub tools: Vec<ToolInfo>,
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
        self.runtime
            .call_function("__oc_config", vec![JsValue::from(json_string(&payload)?)])
            .map(|_| ())
    }

    /// Deliver an event to the `event` hook.
    pub fn event(&self, event: Value) -> Result<(), JsError> {
        let payload = serde_json::json!({ "event": event });
        self.runtime
            .call_function("__oc_event", vec![JsValue::from(json_string(&payload)?)])
            .map(|_| ())
    }

    /// Call the `dispose` hook on every registered hook set.
    pub fn dispose(&self) -> Result<(), JsError> {
        self.async_call("__oc_dispose", serde_json::Value::Null)
            .map(|_| ())
    }

    /// Execute a plugin tool by name. `context` mirrors the reference
    /// `ToolContext`.
    pub fn execute_tool(&self, name: &str, args: Value, context: Value) -> Result<Value, JsError> {
        let payload = serde_json::json!({ "name": name, "args": args, "context": context });
        self.async_call("__oc_tool_execute", payload)
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

    fn async_call(&self, name: &str, payload: Value) -> Result<Value, JsError> {
        let payload = json_string(&payload)?;
        self.runtime.set_global_null("__oc_pending")?;
        self.runtime.call_function(
            "__oc_async_call",
            vec![JsValue::from(name), JsValue::from(payload)],
        )?;
        self.runtime.pump_jobs();
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
                    .unwrap_or("plugin call failed")
                    .to_string();
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
        let runtime = Runtime::new()?;
        runtime.install_bridge(self.host.clone(), self.resolver.clone())?;
        runtime.eval(
            crate::polyfill::RUNTIME_SOURCE,
            "opencode-polyfill-runtime.js",
        )?;
        Ok(LoadedPlugin {
            runtime,
            summary: LoadedSummary {
                hook_names: Vec::new(),
                tools: Vec::new(),
            },
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
