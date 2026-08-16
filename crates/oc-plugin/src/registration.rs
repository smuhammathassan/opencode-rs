//! Host-facing registration and client-RPC contracts for loaded plugins.
//!
//! The JavaScript compatibility layer can discover registrations, but the
//! integrating application still needs a typed hand-off point. These small
//! contracts intentionally carry JSON rather than reproducing the server's
//! command/skill/tool models inside `oc-plugin`.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Registration kind used by a typed host to publish the result of a
/// provider `models` hook. It is deliberately separate from the declarative
/// `provider` registration: the reference hook carries a function, which the
/// current QuickJS bridge cannot serialize or invoke from the provider
/// registry's synchronous construction path.
pub const PROVIDER_MODEL_HOOK_KIND: &str = "provider.models";

/// A registration emitted by a plugin during loading or discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRegistration {
    /// The plugin's exported id, when it provided one.
    #[serde(rename = "pluginId", skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Extensible registration kind (`tool`, `command`, `skill`, `hook`, ...).
    pub kind: String,
    /// The kind-specific JSON payload.
    pub input: Value,
}

impl PluginRegistration {
    pub fn new(
        plugin_id: Option<impl Into<String>>,
        kind: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            plugin_id: plugin_id.map(Into::into),
            kind: kind.into(),
            input,
        }
    }

    /// Build a typed-envelope registration for a provider model-hook result.
    /// The model payload remains JSON at the plugin boundary and is validated
    /// into native `oc-provider` models by the server adapter.
    pub fn provider_model_hook(
        plugin_id: Option<impl Into<String>>,
        provider_id: impl Into<String>,
        models: Value,
    ) -> Self {
        Self::new(
            plugin_id,
            PROVIDER_MODEL_HOOK_KIND,
            serde_json::json!({
                "id": provider_id.into(),
                "models": models,
            }),
        )
    }
}

/// JSON envelope for a provider model-hook result.
///
/// This is the smallest typed hand-off that can cross the current plugin
/// runtime boundary. It represents returned model data, not the executable
/// JavaScript callback itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderModelHookRegistration {
    pub id: String,
    pub models: Value,
}

/// A sink supplied by the integrating application for plugin registrations.
///
/// Returning an error lets a host reject malformed or unsupported
/// registrations while the plugin is being loaded. No server model is
/// assumed here; the sink is the integration seam for that model.
pub trait PluginRegistrationSink: Send + Sync {
    fn register(&self, registration: PluginRegistration) -> Result<(), String>;
}

/// A concrete, thread-safe sink useful for embedding and tests.
#[derive(Clone, Default)]
pub struct InMemoryRegistrationSink {
    registrations: Arc<Mutex<Vec<PluginRegistration>>>,
}

impl InMemoryRegistrationSink {
    pub fn snapshot(&self) -> Vec<PluginRegistration> {
        self.registrations
            .lock()
            .expect("plugin registration sink lock poisoned")
            .clone()
    }

    pub fn registrations_for(&self, kind: &str) -> Vec<PluginRegistration> {
        self.snapshot()
            .into_iter()
            .filter(|registration| registration.kind == kind)
            .collect()
    }
}

impl PluginRegistrationSink for InMemoryRegistrationSink {
    fn register(&self, registration: PluginRegistration) -> Result<(), String> {
        self.registrations
            .lock()
            .map_err(|_| "plugin registration sink lock poisoned".to_string())?
            .push(registration);
        Ok(())
    }
}

/// The stable request envelope for calls from plugin code to the host client.
///
/// `PluginHost::client` remains available for existing hosts; the typed
/// `PluginHost::client_rpc` adapter lets new hosts override one contract
/// without depending on JavaScript's nested client object shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientRpcRequest {
    pub method: String,
    pub args: Value,
}

impl ClientRpcRequest {
    pub fn new(method: impl Into<String>, args: Value) -> Self {
        Self {
            method: method.into(),
            args,
        }
    }
}
