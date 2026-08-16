//! Server-side projection of declarative plugin registrations.
//!
//! The plugin crate deliberately stops at a typed JSON registration contract;
//! this module is the server-owned adapter that layers those registrations on
//! the resolved config consumed by command, provider, and agent handlers.

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::state::AppState;

fn provider_entries(input: &Value) -> Vec<(String, Value)> {
    let Some(object) = input.as_object() else {
        return Vec::new();
    };

    // Provider registrations use `id` as their stable registry key. `name` is
    // the display label and must not silently change which provider is
    // overridden when both fields are present.
    if let Some(id) = object
        .get("id")
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        return vec![(id.to_string(), input.clone())];
    }

    object
        .iter()
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

fn merge_provider_entry(entries: &mut Map<String, Value>, id: String, input: Value) {
    // Keep the plugin boundary JSON-compatible, but require the same typed
    // provider shape consumed by oc-provider before applying model overrides.
    // Invalid registrations retain the historical pass-through behavior and
    // are ignored later by provider_catalog_from_config.
    if serde_json::from_value::<oc_provider::provider::registry::ConfigProvider>(input.clone())
        .is_err()
    {
        entries.insert(id, input);
        return;
    }

    let merged = entries
        .remove(&id)
        .map(|existing| oc_provider::provider::merge_deep(existing, input.clone()))
        .unwrap_or(input);
    entries.insert(id, merged);
}

/// Merge plugin declarations into the config projection used by handlers.
///
/// Registrations are applied after the file/remote config, matching the
/// runtime order in which configured plugins are loaded. Unknown kinds are
/// retained by the plugin sink for future consumers but are intentionally not
/// guessed into server config here.
pub(crate) fn merged_config(state: &AppState, mut config: Value) -> Value {
    let Some(root) = config.as_object_mut() else {
        return config;
    };

    for registration in state.plugin_registrations.snapshot() {
        let section = match registration.kind.as_str() {
            "command" => "command",
            "provider" => "provider",
            "agent" => "agent",
            _ => continue,
        };
        let entries = root
            .entry(section.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(entries) = entries.as_object_mut() else {
            continue;
        };
        let registrations = if section == "provider" {
            provider_entries(&registration.input)
        } else {
            named_entries(&registration.input)
        };
        for (name, mut value) in registrations {
            if section == "command" {
                // The v1 plugin API exposes `command({ name, template, ... })`;
                // command config stores the name as the map key instead.
                if let Some(object) = value.as_object_mut() {
                    object.remove("name");
                }
            }
            if section == "provider" {
                merge_provider_entry(entries, name, value);
            } else {
                entries.insert(name, value);
            }
        }
    }
    config
}

/// Materialize executable `Hooks.provider.models` results emitted by the
/// plugin host. The callback itself cannot cross the QuickJS boundary as a
/// JSON value; once the host has invoked it, this adapter validates the
/// returned model map and feeds it into the native provider registry.
pub(crate) fn plugin_model_hooks(
    state: &AppState,
) -> Vec<oc_provider::provider::registry::ProviderModelHookRegistration> {
    state
        .plugin_registrations
        .snapshot()
        .into_iter()
        .filter(|registration| {
            registration.kind == oc_plugin::registration::PROVIDER_MODEL_HOOK_KIND
        })
        .filter_map(|registration| {
            let object = registration.input.as_object()?;
            let provider_id = object.get("id").and_then(Value::as_str)?.to_string();
            let models = object.get("models")?.as_object()?;
            let mut parsed = IndexMap::new();
            for (model_id, raw) in models {
                let mut model = raw.as_object()?.clone();
                model
                    .entry("id".to_string())
                    .or_insert_with(|| Value::String(model_id.clone()));
                model
                    .entry("providerID".to_string())
                    .or_insert_with(|| Value::String(provider_id.clone()));
                model
                    .entry("api".to_string())
                    .or_insert_with(|| json!({"id": model_id}));
                let model =
                    serde_json::from_value::<oc_provider::provider::Model>(Value::Object(model))
                        .ok()?;
                parsed.insert(model_id.clone(), model);
            }
            Some(
                oc_provider::provider::registry::ProviderModelHookRegistration::new(
                    provider_id,
                    parsed,
                ),
            )
        })
        .collect()
}

/// Return plugin-provided skills in the native skill model so they can be
/// listed and exposed as slash commands alongside filesystem skills.
pub(crate) fn plugin_skill_infos(state: &AppState) -> Vec<oc_command::skill::Info> {
    state
        .plugin_registrations
        .snapshot()
        .into_iter()
        .filter(|registration| registration.kind == "skill")
        .filter_map(|registration| {
            let mut value = registration.input;
            let object = value.as_object_mut()?;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())?
                .to_string();
            object.insert("name".into(), Value::String(name));
            object
                .entry("location")
                .or_insert_with(|| Value::String(plugin_location(&registration.plugin_id)));
            object
                .entry("content")
                .or_insert_with(|| Value::String(String::new()));
            serde_json::from_value(value).ok()
        })
        .collect()
}

pub(crate) fn plugin_skill_values(state: &AppState) -> Vec<Value> {
    plugin_skill_infos(state)
        .into_iter()
        .filter_map(|skill| serde_json::to_value(skill).ok())
        .collect()
}

fn plugin_location(plugin_id: &Option<String>) -> String {
    match plugin_id.as_deref() {
        Some(plugin_id) if !plugin_id.is_empty() => format!("<plugin:{plugin_id}>"),
        _ => "<plugin>".to_string(),
    }
}

fn named_entries(input: &Value) -> Vec<(String, Value)> {
    let Some(object) = input.as_object() else {
        return Vec::new();
    };
    if let Some(name) = object
        .get("name")
        .or_else(|| object.get("id"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    {
        return vec![(name.to_string(), input.clone())];
    }
    object
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{merged_config, plugin_model_hooks};
    use crate::auth::AuthConfig;
    use crate::cors::CorsOptions;
    use crate::location::Location;
    use crate::state::AppState;
    use oc_plugin::{PluginRegistration, PluginRegistrationSink};

    #[test]
    fn plugin_commands_and_agents_layer_over_resolved_config() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        state
            .plugin_registrations
            .register(PluginRegistration::new(
                Some("plugin"),
                "command",
                serde_json::json!({"name": "review", "template": "Review $ARGUMENTS"}),
            ))
            .unwrap();
        state
            .plugin_registrations
            .register(PluginRegistration::new(
                Some("plugin"),
                "agent",
                serde_json::json!({"name": "audit", "description": "Audit code"}),
            ))
            .unwrap();

        let config = merged_config(
            &state,
            serde_json::json!({
                "command": {"old": {"template": "old"}}
            }),
        );
        assert_eq!(config["command"]["review"]["template"], "Review $ARGUMENTS");
        assert_eq!(config["agent"]["audit"]["description"], "Audit code");
        assert!(config["command"]["review"].get("name").is_none());
    }

    #[test]
    fn plugin_provider_model_overrides_deep_merge_over_config() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        state
            .plugin_registrations
            .register(PluginRegistration::new(
                Some("model-plugin"),
                "provider",
                serde_json::json!({
                    "id": "demo",
                    "name": "Plugin Demo",
                    "options": {"headers": {"plugin": "2"}},
                    "models": {
                        "existing": {"name": "Plugin Existing"},
                        "plugin-only": {
                            "name": "Plugin Only",
                            "limit": {"context": 128000}
                        }
                    }
                }),
            ))
            .unwrap();

        let config = merged_config(
            &state,
            serde_json::json!({
                "provider": {
                    "demo": {
                        "name": "Configured Demo",
                        "options": {
                            "baseURL": "https://example.test",
                            "headers": {"configured": "1"}
                        },
                        "models": {
                            "existing": {
                                "name": "Configured Existing",
                                "limit": {"output": 4096}
                            }
                        }
                    }
                }
            }),
        );

        assert_eq!(config["provider"]["demo"]["name"], "Plugin Demo");
        assert_eq!(
            config["provider"]["demo"]["options"]["baseURL"],
            "https://example.test"
        );
        assert_eq!(
            config["provider"]["demo"]["options"]["headers"]["configured"],
            "1"
        );
        assert_eq!(
            config["provider"]["demo"]["options"]["headers"]["plugin"],
            "2"
        );
        assert_eq!(
            config["provider"]["demo"]["models"]["existing"]["name"],
            "Plugin Existing"
        );
        assert_eq!(
            config["provider"]["demo"]["models"]["existing"]["limit"]["output"],
            4096
        );
        assert_eq!(
            config["provider"]["demo"]["models"]["plugin-only"]["limit"]["context"],
            128000
        );

        let providers = crate::handlers::provider::provider_catalog_from_config(&config)
            .expect("typed plugin provider should enter the registry");
        let demo = providers
            .iter()
            .find(|provider| provider.id == "demo")
            .expect("plugin provider should be listed");
        assert_eq!(demo.models["existing"].name, "Plugin Existing");
        assert_eq!(demo.models["existing"].limit.output, 4096.0);
        assert_eq!(demo.models["plugin-only"].limit.context, 128_000.0);
    }

    #[test]
    fn provider_model_hook_registration_materializes_partial_models() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        state
            .plugin_registrations
            .register(PluginRegistration::provider_model_hook(
                Some("model-hook"),
                "openai",
                serde_json::json!({
                    "gpt-hook": { "name": "Hook model", "limit": { "context": 128000 } }
                }),
            ))
            .unwrap();

        let hooks = plugin_model_hooks(&state);
        assert_eq!(hooks.len(), 1);
        let model = hooks[0].models.get("gpt-hook").unwrap();
        assert_eq!(model.id, "gpt-hook");
        assert_eq!(model.provider_id, "openai");
        assert_eq!(model.name, "Hook model");
        assert_eq!(model.limit.context, 128_000.0);
    }
}
