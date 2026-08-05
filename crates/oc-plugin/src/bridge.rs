//! The JS <-> Rust bridge dispatch.
//!
//! JS calls a single Rust callback (`__oc_host_bridge(method, payload)`) for
//! every host service it needs. This module routes each method to the
//! [`PluginHost`] implementation or the [`ModuleResolver`].

use std::sync::Arc;

use serde_json::Value;

use crate::host::PluginHost;
use crate::loader::ModuleResolver;

/// Bridge entrypoint used by the runtime callback.
pub fn dispatch(
    host: &dyn PluginHost,
    resolver: &ModuleResolver,
    method: &str,
    payload: &Value,
) -> Result<Value, String> {
    match method {
        "log" => {
            let level = payload
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info");
            let args = payload
                .get("args")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| match item {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            host.log(level, &args);
            Ok(Value::Null)
        }
        "resolve" => {
            let spec = payload.get("spec").and_then(Value::as_str).unwrap_or("");
            match resolver.resolve(spec) {
                Ok(Some(code)) => Ok(serde_json::json!({ "kind": "inline", "code": code })),
                Ok(None) => Ok(Value::Null),
                Err(err) => Err(err),
            }
        }
        "fetch" => {
            let url = payload.get("url").and_then(Value::as_str).unwrap_or("");
            let method = payload
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET");
            let headers = payload.get("headers").cloned().unwrap_or(Value::Null);
            let body = payload.get("body").and_then(Value::as_str);
            host.fetch(url, method, &headers, body)
        }
        "shell.exec" => host.shell_exec(payload),
        "shell.braces" => {
            let pattern = payload.get("pattern").and_then(Value::as_str).unwrap_or("");
            let values = host.shell_braces(pattern)?;
            Ok(serde_json::to_value(values).map_err(|e| e.to_string())?)
        }
        "fs" => {
            let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
            let args = payload.get("args").cloned().unwrap_or(Value::Null);
            host.fs(method, &args)
        }
        "os" => {
            let name = payload.get("name").and_then(Value::as_str).unwrap_or("");
            Ok(host.os(name).map(Value::String).unwrap_or(Value::Null))
        }
        "client" => {
            let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
            let args = payload.get("args").cloned().unwrap_or(Value::Null);
            host.client(method, &args)
        }
        "tool.metadata" => Ok(Value::Null),
        "tool.ask" => {
            let call_id = payload.get("callID").and_then(Value::as_str).unwrap_or("");
            let input = payload.get("input").cloned().unwrap_or(Value::Null);
            host.tool_ask(call_id, &input)
        }
        "v1.register" => {
            let kind = payload.get("kind").and_then(Value::as_str).unwrap_or("");
            let input = payload.get("input").cloned().unwrap_or(Value::Null);
            host.register_v1(kind, &input);
            Ok(Value::Null)
        }
        "v2.transform" => {
            let domain = payload.get("domain").and_then(Value::as_str).unwrap_or("");
            host.v2_transform_registered(domain);
            Ok(Value::Null)
        }
        "v2.reload" => {
            let domain = payload.get("domain").and_then(Value::as_str).unwrap_or("");
            host.v2_reload(domain);
            Ok(Value::Null)
        }
        other => Err(format!("unknown bridge method '{other}'")),
    }
}

/// The callback closure registered on the runtime as `__oc_host_bridge`.
///
/// JS always receives a JSON string. Dispatch failures are encoded as
/// `{ "__error": message }` so the JS bridge can throw a real `Error`.
pub fn make_callback(
    host: Arc<dyn PluginHost>,
    resolver: Arc<ModuleResolver>,
) -> impl Fn(String, String) -> String + Send + Sync + 'static {
    move |method, payload| {
        let result: Result<Value, String> = serde_json::from_str(&payload)
            .map_err(|e| e.to_string())
            .and_then(|payload| dispatch(host.as_ref(), resolver.as_ref(), &method, &payload));
        match result {
            Ok(value) => serde_json::to_string(&value).unwrap_or_else(|_| "null".into()),
            Err(message) => serde_json::to_string(&serde_json::json!({ "__error": message }))
                .unwrap_or_else(|_| "null".into()),
        }
    }
}
