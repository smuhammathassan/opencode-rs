//! Native default plugins: the data-driven provider catalog transforms and the
//! built-in skill registration that the reference ships as internal plugins.
//!
//! Mirrors the *feasible* entries of `reference/packages/core/src/plugin/provider/*.ts`
//! (those that only reshape the provider catalog and need no live service or
//! JS AI-SDK runtime) and `reference/packages/core/src/plugin/skill.ts` (the
//! built-in `customize-opencode` skill). The reference runs these through the
//! v2 `catalog`/`skill` transform bridge; this module applies the same
//! semantics to the JSON catalog draft so an embedding application can apply
//! them natively (for example alongside the native auth registry in
//! oc-server/src/builtin_auth.rs).

use serde_json::{json, Value};

use crate::registration::PluginRegistration;

/// The v2 catalog draft shape these transforms operate on. Each provider item
/// is `{ "provider": { id, api: { type, package, url }, request: { headers },
/// disabled }, "models": { id: { enabled, ... } } }` — the same shape the
/// polyfill's `catalog` domain wrapper exposes to JS plugins.
pub const CATALOG_PROVIDERS_KEY: &str = "providers";

/// How a header mutation is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderMode {
    /// `provider.request.headers[name] = value`
    Set,
    /// `provider.request.headers[name] ??= value` (set only when absent)
    SetIfAbsent,
}

/// One native default provider plugin: a catalog transform applied to every
/// provider whose API matches the descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPlugin {
    /// The reference plugin id (also the `define({ id })` value).
    pub id: &'static str,
    /// The AI-SDK package the provider must advertise (`api.package`).
    pub api_package: &'static str,
    /// When present, the provider API URL must match exactly (`api.url`).
    pub api_url: Option<&'static str>,
    /// Skip providers whose `disabled` flag is set (llmgateway behavior).
    pub skip_disabled: bool,
    /// Headers to set on `provider.request.headers`.
    pub headers: Vec<(&'static str, &'static str, HeaderMode)>,
    /// Model ids to disable (`models[id].enabled = false`) when present.
    pub disable_models: Vec<&'static str>,
}

const OPENCODE_URL: &str = "https://opencode.ai/";

/// The feasible default provider plugins ported from
/// `reference/packages/core/src/plugin/provider/*.ts`. Entries that need a JS
/// AI-SDK runtime (`aisdk.sdk` hooks, `import("@ai-sdk/...")`) are out of
/// scope for the in-process host and are not included.
pub fn default_provider_plugins() -> Vec<ProviderPlugin> {
    vec![
        ProviderPlugin {
            id: "nvidia",
            api_package: "@ai-sdk/openai-compatible",
            api_url: Some("https://integrate.api.nvidia.com/v1"),
            skip_disabled: false,
            headers: vec![
                ("HTTP-Referer", OPENCODE_URL, HeaderMode::Set),
                ("X-Title", "opencode", HeaderMode::Set),
                (
                    "X-BILLING-INVOKE-ORIGIN",
                    "OpenCode",
                    HeaderMode::SetIfAbsent,
                ),
            ],
            disable_models: vec![],
        },
        ProviderPlugin {
            id: "kilo",
            api_package: "@ai-sdk/openai-compatible",
            api_url: Some("https://api.kilo.ai/api/gateway"),
            skip_disabled: false,
            headers: vec![
                ("HTTP-Referer", OPENCODE_URL, HeaderMode::Set),
                ("X-Title", "opencode", HeaderMode::Set),
            ],
            disable_models: vec![],
        },
        ProviderPlugin {
            id: "llmgateway",
            api_package: "@ai-sdk/openai-compatible",
            api_url: Some("https://api.llmgateway.io/v1"),
            skip_disabled: true,
            headers: vec![
                ("HTTP-Referer", OPENCODE_URL, HeaderMode::Set),
                ("X-Title", "opencode", HeaderMode::Set),
                ("X-Source", "opencode", HeaderMode::Set),
            ],
            disable_models: vec![],
        },
        ProviderPlugin {
            id: "cerebras",
            api_package: "@ai-sdk/cerebras",
            api_url: None,
            skip_disabled: false,
            headers: vec![(
                "X-Cerebras-3rd-Party-Integration",
                "opencode",
                HeaderMode::Set,
            )],
            disable_models: vec![],
        },
        ProviderPlugin {
            id: "openrouter",
            api_package: "@openrouter/ai-sdk-provider",
            api_url: None,
            skip_disabled: false,
            headers: vec![
                ("HTTP-Referer", OPENCODE_URL, HeaderMode::Set),
                ("X-Title", "opencode", HeaderMode::Set),
            ],
            disable_models: vec!["gpt-5-chat-latest".into(), "openai/gpt-5-chat".into()],
        },
        ProviderPlugin {
            id: "anthropic",
            api_package: "@ai-sdk/anthropic",
            api_url: None,
            skip_disabled: false,
            headers: vec![(
                "anthropic-beta",
                "interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14",
                HeaderMode::Set,
            )],
            disable_models: vec![],
        },
    ]
}

/// Look up a default provider plugin by its reference id.
pub fn provider_plugin(id: &str) -> Option<ProviderPlugin> {
    default_provider_plugins()
        .into_iter()
        .find(|plugin| plugin.id == id)
}

/// Apply one provider plugin's catalog transform to a mutable catalog draft.
/// Non-matching providers are left untouched; headers are set on
/// `provider.request.headers` and matching model ids are disabled.
pub fn apply_provider_plugin(draft: &mut Value, plugin: &ProviderPlugin) {
    let Some(providers) = draft
        .get_mut(CATALOG_PROVIDERS_KEY)
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for item in providers.iter_mut() {
        let Some(provider) = item.get_mut("provider") else {
            continue;
        };
        let Some(api) = provider.get("api") else {
            continue;
        };
        if api.get("type").and_then(Value::as_str) != Some("aisdk") {
            continue;
        }
        if api.get("package").and_then(Value::as_str) != Some(plugin.api_package) {
            continue;
        }
        if let Some(expected) = plugin.api_url {
            if api.get("url").and_then(Value::as_str) != Some(expected) {
                continue;
            }
        }
        if plugin.skip_disabled
            && provider
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }

        // Headers: mirror `provider.request.headers[name] = value` and the
        // `??=` set-if-absent variant.
        if provider.get("request").is_none() {
            provider["request"] = json!({});
        }
        let request = provider
            .get_mut("request")
            .and_then(Value::as_object_mut)
            .expect("request object was just created");
        if request.get("headers").is_none() {
            request.insert("headers".to_string(), json!({}));
        }
        let headers = request
            .get_mut("headers")
            .and_then(Value::as_object_mut)
            .expect("headers object was just created");
        for (name, value, mode) in &plugin.headers {
            if *mode == HeaderMode::SetIfAbsent && headers.contains_key(*name) {
                continue;
            }
            headers.insert((*name).to_string(), Value::String((*value).to_string()));
        }

        // Disable model aliases that must not resolve on the generic path.
        if !plugin.disable_models.is_empty() {
            if let Some(models) = item.get_mut("models").and_then(Value::as_object_mut) {
                for model_id in plugin.disable_models.iter().copied() {
                    if let Some(model) = models.get_mut(model_id) {
                        if model.is_object() {
                            model["enabled"] = json!(false);
                        }
                    }
                }
            }
        }
    }
}

/// Apply every feasible default provider plugin to a catalog draft (the
/// "default plugins set" the reference loads for provider catalog shaping).
pub fn apply_all_provider_plugins(draft: &mut Value) {
    for plugin in default_provider_plugins() {
        apply_provider_plugin(draft, &plugin);
    }
}

// ---------------------------------------------------------------------------
// Built-in skill (reference/packages/core/src/plugin/skill.ts)
// ---------------------------------------------------------------------------

/// The reference built-in skill registration id.
pub const CUSTOMIZE_OPENCODE_SKILL_NAME: &str = "customize-opencode";

/// Build the `PluginRegistration` for the built-in `customize-opencode` skill.
/// The embedding application supplies the canonical markdown content (the
/// reference keeps it in packages/core/src/plugin/skill/customize-opencode.md;
/// oc-command ships the Rust copy).
pub fn customize_opencode_skill_registration(
    plugin_id: Option<&str>,
    content: &str,
) -> PluginRegistration {
    let input = json!({
        "name": CUSTOMIZE_OPENCODE_SKILL_NAME,
        "description": "Use ONLY when the user is editing or creating opencode's own configuration: opencode.json, opencode.jsonc, files under .opencode/, or files under ~/.config/opencode/. Also use when creating or fixing opencode agents, subagents, commands, skills, plugins, MCP servers, or permission rules. Do not use for the user's own application code, or for any project that is not configuring opencode itself.",
        "location": "/builtin/customize-opencode.md",
        "content": content,
    });
    PluginRegistration::new(plugin_id, "skill", input)
}

/// Apply the built-in skill source to a mutable v2 `skill` domain draft, the
/// same shape `ctx.skill.transform((draft) => draft.source(...))` produces
/// (reference/packages/core/src/plugin/skill.ts).
pub fn apply_default_skill_source(draft: &mut Value, content: &str) {
    if draft.get("sources").is_none() {
        draft["sources"] = json!([]);
    }
    let sources = draft
        .get_mut("sources")
        .and_then(Value::as_array_mut)
        .expect("sources array was just created");
    let source = json!({
        "type": "embedded",
        "skill": {
            "name": CUSTOMIZE_OPENCODE_SKILL_NAME,
            "description": "Use ONLY when the user is editing or creating opencode's own configuration: opencode.json, opencode.jsonc, files under .opencode/, or files under ~/.config/opencode/. Also use when creating or fixing opencode agents, subagents, commands, skills, plugins, MCP servers, or permission rules. Do not use for the user's own application code, or for any project that is not configuring opencode itself.",
            "location": "/builtin/customize-opencode.md",
            "content": content,
        },
    });
    sources.push(source);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_item(id: &str, package: &str, url: &str, disabled: bool) -> Value {
        json!({
            "provider": {
                "id": id,
                "api": { "type": "aisdk", "package": package, "url": url },
                "request": { "headers": {} },
                "disabled": disabled,
            },
            "models": {
                "gpt-5-chat-latest": { "enabled": true },
                "openai/gpt-5-chat": { "enabled": true },
                "other-model": { "enabled": true },
            }
        })
    }

    #[test]
    fn registry_includes_feasible_provider_plugins() {
        let ids: Vec<&str> = default_provider_plugins()
            .iter()
            .map(|plugin| plugin.id)
            .collect();
        for expected in [
            "nvidia",
            "kilo",
            "llmgateway",
            "cerebras",
            "openrouter",
            "anthropic",
        ] {
            assert!(ids.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn nvidia_transform_sets_gateway_headers() {
        let plugin = provider_plugin("nvidia").expect("nvidia plugin");
        let mut draft = json!({
            "providers": [
                provider_item("nv", "@ai-sdk/openai-compatible", "https://integrate.api.nvidia.com/v1", false),
                provider_item("other", "@ai-sdk/openai-compatible", "https://other.test/v1", false),
            ]
        });
        apply_provider_plugin(&mut draft, &plugin);
        let nvidia = &draft["providers"][0]["provider"];
        assert_eq!(
            nvidia["request"]["headers"]["HTTP-Referer"],
            "https://opencode.ai/"
        );
        assert_eq!(nvidia["request"]["headers"]["X-Title"], "opencode");
        assert_eq!(
            nvidia["request"]["headers"]["X-BILLING-INVOKE-ORIGIN"],
            "OpenCode"
        );
        // Non-matching provider stays untouched.
        assert_eq!(
            draft["providers"][1]["provider"]["request"]["headers"],
            json!({})
        );
    }

    #[test]
    fn openrouter_transform_disables_alias_models() {
        let plugin = provider_plugin("openrouter").expect("openrouter plugin");
        let mut draft = json!({
            "providers": [provider_item("or", "@openrouter/ai-sdk-provider", "https://openrouter.ai/api/v1", false)]
        });
        apply_provider_plugin(&mut draft, &plugin);
        assert_eq!(
            draft["providers"][0]["models"]["gpt-5-chat-latest"]["enabled"],
            false
        );
        assert_eq!(
            draft["providers"][0]["models"]["openai/gpt-5-chat"]["enabled"],
            false
        );
        assert_eq!(
            draft["providers"][0]["models"]["other-model"]["enabled"],
            true
        );
        assert_eq!(
            draft["providers"][0]["provider"]["request"]["headers"]["X-Title"],
            "opencode"
        );
    }

    #[test]
    fn anthropic_transform_sets_beta_header() {
        let plugin = provider_plugin("anthropic").expect("anthropic plugin");
        let mut draft = json!({
            "providers": [provider_item("anthropic", "@ai-sdk/anthropic", "https://api.anthropic.com", false)]
        });
        apply_provider_plugin(&mut draft, &plugin);
        let beta = draft["providers"][0]["provider"]["request"]["headers"]["anthropic-beta"]
            .as_str()
            .unwrap();
        assert!(beta.contains("interleaved-thinking-2025-05-14"));
        assert!(beta.contains("fine-grained-tool-streaming-2025-05-14"));
    }

    #[test]
    fn llmgateway_transform_skips_disabled_providers() {
        let plugin = provider_plugin("llmgateway").expect("llmgateway plugin");
        let mut draft = json!({
            "providers": [
                provider_item("llm", "@ai-sdk/openai-compatible", "https://api.llmgateway.io/v1", true),
                provider_item("llm2", "@ai-sdk/openai-compatible", "https://api.llmgateway.io/v1", false),
            ]
        });
        apply_provider_plugin(&mut draft, &plugin);
        // Disabled provider is skipped.
        assert_eq!(
            draft["providers"][0]["provider"]["request"]["headers"],
            json!({})
        );
        // Enabled provider gets the headers.
        assert_eq!(
            draft["providers"][1]["provider"]["request"]["headers"]["X-Source"],
            "opencode"
        );
    }

    #[test]
    fn set_if_absent_does_not_override_existing_header() {
        let plugin = provider_plugin("nvidia").expect("nvidia plugin");
        let mut item = provider_item(
            "nv",
            "@ai-sdk/openai-compatible",
            "https://integrate.api.nvidia.com/v1",
            false,
        );
        item["provider"]["request"]["headers"]["X-BILLING-INVOKE-ORIGIN"] = json!("Existing");
        let mut draft = json!({ "providers": [item] });
        apply_provider_plugin(&mut draft, &plugin);
        assert_eq!(
            draft["providers"][0]["provider"]["request"]["headers"]["X-BILLING-INVOKE-ORIGIN"],
            "Existing"
        );
    }

    #[test]
    fn skill_registration_and_draft_transform_have_expected_shape() {
        let content = "# Customizing opencode\nsummary";
        let registration = customize_opencode_skill_registration(Some("skill"), content);
        assert_eq!(registration.kind, "skill");
        assert_eq!(registration.input["name"], CUSTOMIZE_OPENCODE_SKILL_NAME);
        assert_eq!(registration.input["content"], content);
        assert!(registration.input["description"]
            .as_str()
            .unwrap()
            .contains("opencode's own configuration"));

        let mut draft = json!({});
        apply_default_skill_source(&mut draft, content);
        assert_eq!(
            draft["sources"][0]["skill"]["name"],
            CUSTOMIZE_OPENCODE_SKILL_NAME
        );
        assert_eq!(draft["sources"][0]["skill"]["content"], content);
        assert_eq!(draft["sources"][0]["type"], "embedded");
    }
}
