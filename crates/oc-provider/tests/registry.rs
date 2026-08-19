#![allow(clippy::field_reassign_with_default)]
//! Golden tests for the provider registry: models.dev snapshot parsing and
//! the `build_registry` merge logic.
//!
//! Expected values are derived from `data/models.json` (a snapshot of
//! `https://models.opencode.ai/api.json`) and the conversion logic in
//! `provider.ts`.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use oc_provider::models_dev;
use oc_provider::provider::registry::{build_registry, ConfigInput, ConfigProvider, RegistryInput};
use oc_provider::provider::{
    self, default_model_ids, from_models_dev_provider, parse_model, sort, Source,
};

fn snapshot() -> IndexMap<String, models_dev::Provider> {
    models_dev::snapshot().unwrap()
}

#[test]
fn snapshot_has_expected_providers() {
    let catalog = snapshot();
    assert_eq!(catalog.len(), 180);
    for provider_id in [
        "openai",
        "anthropic",
        "google",
        "google-vertex",
        "openrouter",
        "deepseek",
        "zhipuai",
        "opencode",
        "amazon-bedrock",
        "azure",
        "xai",
        "mistral",
        "github-copilot",
    ] {
        assert!(catalog.contains_key(provider_id), "missing {}", provider_id);
    }
    assert_eq!(catalog["openai"].name, "OpenAI");
    assert_eq!(catalog["openai"].npm.as_deref(), Some("@ai-sdk/openai"));
    assert_eq!(catalog["openai"].env, vec!["OPENAI_API_KEY"]);
    assert_eq!(
        catalog["deepseek"].api.as_deref(),
        Some("https://api.deepseek.com")
    );
    assert_eq!(
        catalog["deepseek"].npm.as_deref(),
        Some("@ai-sdk/openai-compatible")
    );
    assert_eq!(catalog["opencode"].name, "OpenCode Zen");
}

#[test]
fn snapshot_converts_to_provider_info() {
    let catalog = snapshot();
    let openai = from_models_dev_provider(&catalog["openai"]);
    assert_eq!(openai.id, "openai");
    assert_eq!(openai.source, Source::Custom);
    assert_eq!(openai.name, "OpenAI");
    assert_eq!(openai.env, vec!["OPENAI_API_KEY"]);
    assert_eq!(openai.models.len(), 58);

    let gpt_5_2_pro = &openai.models["gpt-5.2-pro"];
    assert_eq!(gpt_5_2_pro.api.id, "gpt-5.2-pro");
    assert_eq!(gpt_5_2_pro.api.npm, "@ai-sdk/openai");
    assert_eq!(gpt_5_2_pro.api.url, "");
    assert_eq!(gpt_5_2_pro.limit.context, 400_000.0);
    assert_eq!(gpt_5_2_pro.limit.input, Some(272_000.0));
    assert_eq!(gpt_5_2_pro.limit.output, 128_000.0);
    assert!(gpt_5_2_pro.capabilities.reasoning);
    assert!(gpt_5_2_pro.capabilities.toolcall);
    assert_eq!(gpt_5_2_pro.status, provider::ModelStatus::Active);
    assert!(
        !gpt_5_2_pro.variants.is_empty(),
        "gpt-5.2-pro should have reasoning variants"
    );

    let deepseek = from_models_dev_provider(&catalog["deepseek"]);
    let chat = &deepseek.models["deepseek-chat"];
    assert_eq!(chat.api.id, "deepseek-chat");
    assert_eq!(chat.limit.context, 1_000_000.0);
    assert!(!chat.capabilities.reasoning);
    // deepseek-chat carries no interleaved metadata in the catalog
    assert_eq!(
        chat.capabilities.interleaved,
        provider::InterleavedField::Bool(false)
    );

    let anthropic = from_models_dev_provider(&catalog["anthropic"]);
    let sonnet = &anthropic.models["claude-sonnet-4-6"];
    assert_eq!(sonnet.api.npm, "@ai-sdk/anthropic");
    assert_eq!(sonnet.limit.context, 1_000_000.0);
    assert_eq!(sonnet.limit.output, 128_000.0);
    // claude-sonnet-4-6 is adaptive (modern) -> low/medium/high/max
    let keys: Vec<&str> = sonnet.variants.keys().map(|k| k.as_str()).collect();
    assert_eq!(keys, vec!["low", "medium", "high", "max"]);
}

#[test]
fn cost_conversion_defaults() {
    let catalog = snapshot();
    let zhipuai = from_models_dev_provider(&catalog["zhipuai"]);
    let glm5 = &zhipuai.models["glm-5"];
    assert_eq!(glm5.cost.input, 1.0);
    assert_eq!(glm5.cost.output, 3.2);
    assert_eq!(glm5.cost.cache.read, 0.2);
    assert_eq!(glm5.cost.cache.write, 0.0);
}

#[test]
fn experimental_modes_expand_models() {
    // openai gpt-5.6-sol has experimental modes fast/pro, adding -fast/-pro.
    let catalog = snapshot();
    let openai = from_models_dev_provider(&catalog["openai"]);
    let fast = &openai.models["gpt-5.6-sol-fast"];
    assert_eq!(fast.api.id, "gpt-5.6-sol");
    assert_eq!(fast.name, "GPT-5.6 Sol Fast");
    // fast mode cost overrides the base cost
    assert_eq!(fast.cost.input, 10.0);
    assert_eq!(fast.cost.output, 60.0);
    assert_eq!(fast.cost.cache.read, 1.0);
    assert_eq!(fast.cost.cache.write, 12.5);
    // snake_case body keys become camelCase options
    assert_eq!(fast.options["serviceTier"], "priority");
    // base cost keeps its context_over_200k entry
    assert_eq!(fast.cost.experimental_over_200k.unwrap().input, 10.0);

    let pro = &openai.models["gpt-5.6-sol-pro"];
    assert_eq!(pro.api.id, "gpt-5.6-sol");
    // pro mode has no cost override
    assert_eq!(pro.cost.input, 5.0);
    // reasoning.mode is rewritten to reasoningMode for @ai-sdk/openai
    assert_eq!(pro.options["reasoningMode"], "pro");
}

#[test]
fn models_dev_normalization_fills_defaults() {
    let provider = models_dev::Provider {
        id: "gateway".to_string(),
        name: "Gateway".to_string(),
        env: Vec::new(),
        npm: None,
        api: None,
        models: IndexMap::from_iter([(
            "gpt-5.4".to_string(),
            models_dev::Model {
                id: "gpt-5.4".to_string(),
                name: "GPT-5.4".to_string(),
                family: Some("gpt".to_string()),
                interleaved: Some(models_dev::Interleaved::Field("reasoning_text".to_string())),
                cost: Some(models_dev::Cost {
                    input: Some(2.5),
                    output: Some(15.0),
                    ..Default::default()
                }),
                limit: Some(models_dev::Limit {
                    context: Some(1_050_000.0),
                    input: Some(922_000.0),
                    output: Some(128_000.0),
                }),
                ..Default::default()
            },
        )]),
    };
    let info = from_models_dev_provider(&provider);
    let model = &info.models["gpt-5.4"];
    assert_eq!(model.api.url, "");
    assert_eq!(model.api.npm, "@ai-sdk/openai-compatible");
    assert!(!model.capabilities.temperature);
    assert!(!model.capabilities.reasoning);
    assert!(!model.capabilities.attachment);
    assert!(model.capabilities.toolcall);
    assert_eq!(
        model.capabilities.interleaved,
        provider::InterleavedField::Field {
            field: "reasoning_text".to_string()
        }
    );
    assert_eq!(model.release_date, "");
}

#[test]
fn models_dev_reasoning_options_drive_variants() {
    let provider = models_dev::Provider {
        id: "reasoning".to_string(),
        name: "Reasoning".to_string(),
        env: Vec::new(),
        npm: Some("@ai-sdk/openai".to_string()),
        api: None,
        models: IndexMap::from_iter([
            (
                "explicit".to_string(),
                models_dev::Model {
                    id: "gpt-5.4".to_string(),
                    name: "Explicit".to_string(),
                    reasoning: Some(true),
                    reasoning_options: Some(vec![models_dev::ReasoningOption::Effort {
                        values: vec![Some("low".to_string())],
                    }]),
                    limit: Some(models_dev::Limit {
                        context: Some(128_000.0),
                        output: Some(64_000.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            (
                "empty".to_string(),
                models_dev::Model {
                    id: "gpt-5.4".to_string(),
                    name: "Empty".to_string(),
                    reasoning: Some(true),
                    reasoning_options: Some(vec![]),
                    limit: Some(models_dev::Limit {
                        context: Some(128_000.0),
                        output: Some(64_000.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            (
                "fallback".to_string(),
                models_dev::Model {
                    id: "gpt-5.4".to_string(),
                    name: "Fallback".to_string(),
                    reasoning: Some(true),
                    reasoning_options: Some(vec![models_dev::ReasoningOption::Toggle]),
                    limit: Some(models_dev::Limit {
                        context: Some(128_000.0),
                        output: Some(64_000.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            (
                "override".to_string(),
                models_dev::Model {
                    id: "gemini-3-pro".to_string(),
                    name: "Override".to_string(),
                    reasoning: Some(true),
                    reasoning_options: Some(vec![models_dev::ReasoningOption::Effort {
                        values: vec![Some("high".to_string())],
                    }]),
                    provider: Some(models_dev::ProviderRef {
                        npm: Some("@ai-sdk/google".to_string()),
                        api: None,
                    }),
                    limit: Some(models_dev::Limit {
                        context: Some(128_000.0),
                        output: Some(64_000.0),
                        ..Default::default()
                    }),
                    experimental: Some(models_dev::Experimental {
                        modes: Some(IndexMap::from_iter([(
                            "fast".to_string(),
                            models_dev::Mode::default(),
                        )])),
                    }),
                    ..Default::default()
                },
            ),
            (
                "anthropicCompatible".to_string(),
                models_dev::Model {
                    id: "k3".to_string(),
                    name: "Anthropic Compatible".to_string(),
                    reasoning: Some(true),
                    reasoning_options: Some(vec![models_dev::ReasoningOption::Effort {
                        values: vec![Some("max".to_string())],
                    }]),
                    provider: Some(models_dev::ProviderRef {
                        npm: Some("@ai-sdk/anthropic".to_string()),
                        api: None,
                    }),
                    limit: Some(models_dev::Limit {
                        context: Some(1_048_576.0),
                        output: Some(131_072.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
        ]),
    };
    let models = from_models_dev_provider(&provider).models;
    assert_eq!(
        models["explicit"].variants["low"],
        serde_json::json!({
            "reasoningEffort": "low",
            "reasoningSummary": "auto",
            "include": ["reasoning.encrypted_content"],
        })
        .as_object()
        .unwrap()
        .clone()
    );
    assert!(models["empty"].variants.is_empty());
    let fallback_keys: Vec<&str> = models["fallback"]
        .variants
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        fallback_keys,
        vec!["none", "low", "medium", "high", "xhigh"]
    );
    assert_eq!(
        models["override"].variants["high"],
        serde_json::json!({ "thinkingConfig": { "includeThoughts": true, "thinkingLevel": "high" } })
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!(
        models["anthropicCompatible"].variants["max"],
        serde_json::json!({ "effort": "max" })
            .as_object()
            .unwrap()
            .clone()
    );
    // experimental mode variant reuses the model's reasoning variants
    assert_eq!(
        models["gemini-3-pro-fast"].variants,
        models["override"].variants
    );
}

#[test]
fn sort_prioritizes_preferred_models() {
    let models = vec![
        provider::Model {
            id: "gpt-4o".to_string(),
            ..Default::default()
        },
        provider::Model {
            id: "claude-sonnet-4-5-latest".to_string(),
            ..Default::default()
        },
        provider::Model {
            id: "claude-sonnet-4".to_string(),
            ..Default::default()
        },
        provider::Model {
            id: "gpt-5".to_string(),
            ..Default::default()
        },
    ];
    let sorted = sort(models);
    let ids: Vec<&str> = sorted.iter().map(|m| m.id.as_str()).collect();
    // priority: gpt-5 first, then claude-sonnet-4, latest first within family
    assert_eq!(
        ids,
        vec![
            "gpt-5",
            "claude-sonnet-4-5-latest",
            "claude-sonnet-4",
            "gpt-4o"
        ]
    );
}

#[test]
fn default_model_ids_uses_sorted_first() {
    let catalog = snapshot();
    let providers: IndexMap<String, provider::Info> = catalog
        .iter()
        .take(3)
        .map(|(id, p)| (id.clone(), from_models_dev_provider(p)))
        .collect();
    let defaults = default_model_ids(&providers);
    assert_eq!(defaults.len(), 3);
    let (provider_id, model_id) = defaults.first().unwrap();
    let info = &providers[provider_id];
    assert!(info.models.contains_key(model_id));
}

#[test]
fn parse_model_handles_slashes() {
    assert_eq!(
        parse_model("anthropic/claude-sonnet-4"),
        ("anthropic".to_string(), "claude-sonnet-4".to_string())
    );
    assert_eq!(
        parse_model("openrouter/anthropic/claude-3-opus"),
        (
            "openrouter".to_string(),
            "anthropic/claude-3-opus".to_string()
        )
    );
}

#[test]
fn build_registry_merges_env_and_api_keys() {
    let catalog = snapshot();
    let provider_config = IndexMap::new();
    let mut config = ConfigInput::default();
    config.provider = &provider_config;
    let mut envs = BTreeMap::new();
    envs.insert("OPENAI_API_KEY".to_string(), Some("sk-env".to_string()));
    let mut auths = BTreeMap::new();
    auths.insert(
        "anthropic".to_string(),
        oc_provider::auth::Info::Api(oc_provider::auth::Api {
            key: "sk-anthropic".to_string(),
            metadata: None,
        }),
    );

    let input = RegistryInput {
        catalog: &catalog,
        config,
        envs: &envs,
        auths: &auths,
        enable_experimental_models: false,
    };
    let providers = build_registry(&input).unwrap();

    assert!(
        providers.contains_key("openai"),
        "env key should load openai"
    );
    assert_eq!(providers["openai"].source, Source::Env);
    assert_eq!(providers["openai"].key.as_deref(), Some("sk-env"));
    assert!(
        providers.contains_key("anthropic"),
        "api key should load anthropic"
    );
    assert_eq!(providers["anthropic"].source, Source::Api);
    assert_eq!(providers["anthropic"].key.as_deref(), Some("sk-anthropic"));
}

#[test]
fn build_registry_ignores_empty_environment_credentials() {
    let catalog = snapshot();
    let mut envs = BTreeMap::new();
    envs.insert("OPENAI_API_KEY".to_string(), Some(String::new()));

    let input = RegistryInput {
        catalog: &catalog,
        config: ConfigInput::default(),
        envs: &envs,
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };

    let providers = build_registry(&input).unwrap();
    assert!(
        !providers.contains_key("openai"),
        "an empty env value must not connect a provider"
    );
}

#[test]
fn build_registry_respects_disabled_providers() {
    let catalog = snapshot();
    let provider_config = IndexMap::new();
    let disabled = vec!["anthropic".to_string()];
    let mut config = ConfigInput::default();
    config.provider = &provider_config;
    config.disabled_providers = Some(&disabled);
    let input = RegistryInput {
        catalog: &catalog,
        config,
        envs: &BTreeMap::new(),
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let providers = build_registry(&input).unwrap();
    assert!(!providers.contains_key("anthropic"));
}

#[test]
fn build_registry_filters_alpha_and_deprecated() {
    let catalog = snapshot();
    // opencode has a deprecated model (trinity-large-preview-free) and alpha
    // models gated by enable_experimental_models.
    let _provider_config: IndexMap<String, ConfigProvider> = IndexMap::new();
    let mut envs = BTreeMap::new();
    envs.insert("OPENCODE_API_KEY".to_string(), Some("key".to_string()));

    let strict = RegistryInput {
        catalog: &catalog,
        config: ConfigInput::default(),
        envs: &envs,
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let providers = build_registry(&strict).unwrap();
    let opencode = &providers["opencode"];
    assert!(
        !opencode.models.contains_key("trinity-large-preview-free"),
        "deprecated models are filtered"
    );

    let lenient = RegistryInput {
        catalog: &catalog,
        config: ConfigInput::default(),
        envs: &envs,
        auths: &BTreeMap::new(),
        enable_experimental_models: true,
    };
    let providers = build_registry(&lenient).unwrap();
    assert!(providers["opencode"].models.len() >= opencode.models.len());
}

#[test]
fn build_registry_applies_custom_loader_options() {
    let catalog = snapshot();
    let provider_config = IndexMap::new();
    let mut config = ConfigInput::default();
    config.provider = &provider_config;
    let mut envs = BTreeMap::new();
    envs.insert("ANTHROPIC_API_KEY".to_string(), Some("key".to_string()));
    let input = RegistryInput {
        catalog: &catalog,
        config,
        envs: &envs,
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let providers = build_registry(&input).unwrap();
    // anthropic custom loader adds the anthropic-beta header
    assert_eq!(
        providers["anthropic"].options["headers"]["anthropic-beta"],
        "interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14"
    );
}

#[test]
fn build_registry_config_provider_merges_models() {
    let catalog = snapshot();
    let mut config_providers = IndexMap::new();
    config_providers.insert(
        "my-anthropic".to_string(),
        ConfigProvider {
            name: Some("My Anthropic".to_string()),
            npm: Some("@ai-sdk/anthropic".to_string()),
            api: Some("https://api.anthropic.com".to_string()),
            models: Some(IndexMap::from_iter([(
                "claude-custom".to_string(),
                provider::registry::ConfigModel {
                    name: Some("Claude Custom".to_string()),
                    limit: Some(provider::registry::ConfigLimit {
                        context: Some(200_000.0),
                        output: Some(16_384.0),
                        ..Default::default()
                    }),
                    cost: Some(provider::registry::ConfigCost {
                        input: Some(3.0),
                        output: Some(15.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )])),
            ..Default::default()
        },
    );
    let config = ConfigInput {
        provider: &config_providers,
        ..Default::default()
    };
    let input = RegistryInput {
        catalog: &catalog,
        config,
        envs: &BTreeMap::new(),
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let providers = build_registry(&input).unwrap();
    let provider_info = &providers["my-anthropic"];
    assert_eq!(provider_info.name, "My Anthropic");
    assert_eq!(provider_info.source, Source::Config);
    let model = &provider_info.models["claude-custom"];
    assert_eq!(model.api.npm, "@ai-sdk/anthropic");
    assert_eq!(model.api.url, "https://api.anthropic.com");
    assert_eq!(model.limit.context, 200_000.0);
    assert_eq!(model.limit.output, 16_384.0);
    assert_eq!(model.cost.input, 3.0);
}

#[test]
fn build_registry_custom_source_patch_preserves_models() {
    // opencode with no key: the custom loader deletes free (cost.input == 0)
    // models and sets apiKey "public".
    let catalog = snapshot();
    let input = RegistryInput {
        catalog: &catalog,
        config: ConfigInput::default(),
        envs: &BTreeMap::new(),
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let providers = build_registry(&input).unwrap();
    let opencode = &providers["opencode"];
    assert_eq!(opencode.options["apiKey"], "public");
    for model in opencode.models.values() {
        assert!(model.cost.input > 0.0, "free models removed without auth");
    }
}

/// A fixture npm-metadata resolver: package name -> advertised API base URL.
struct FixtureNpmMetadata(&'static [(&'static str, &'static str)]);

impl oc_provider::provider::registry::NpmMetadata for FixtureNpmMetadata {
    fn provider_base_url(&self, npm: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(name, _)| *name == npm)
            .map(|(_, url)| url.to_string())
    }
}

#[test]
fn build_registry_with_npm_metadata_resolves_api_url() {
    // A config-declared provider that names an npm SDK but supplies no
    // explicit `api` URL and has no models.dev entry resolves its base URL
    // from the injected package-metadata seam.
    let catalog = snapshot();
    let mut config_providers = IndexMap::new();
    config_providers.insert(
        "npm-only-provider".to_string(),
        ConfigProvider {
            name: Some("Npm Only".to_string()),
            npm: Some("@ai-sdk/custom-sdk".to_string()),
            models: Some(IndexMap::from_iter([(
                "custom-1".to_string(),
                provider::registry::ConfigModel {
                    name: Some("Custom One".to_string()),
                    ..Default::default()
                },
            )])),
            ..Default::default()
        },
    );
    let config = ConfigInput {
        provider: &config_providers,
        ..Default::default()
    };
    let input = RegistryInput {
        catalog: &catalog,
        config,
        envs: &BTreeMap::new(),
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let resolver = FixtureNpmMetadata(&[("@ai-sdk/custom-sdk", "https://custom.example/v1")]);

    // Without a resolver the model has no base URL.
    let providers = build_registry(&input).unwrap();
    let model = &providers["npm-only-provider"].models["custom-1"];
    assert_eq!(model.api.npm, "@ai-sdk/custom-sdk");
    assert_eq!(model.api.url, "");

    // With a resolver the base URL falls back to the package metadata.
    let providers = oc_provider::provider::registry::build_registry_with_npm_metadata(
        &input,
        &[],
        Some(&resolver),
    )
    .unwrap();
    let model = &providers["npm-only-provider"].models["custom-1"];
    assert_eq!(model.api.npm, "@ai-sdk/custom-sdk");
    assert_eq!(model.api.url, "https://custom.example/v1");

    // An explicit config `api` URL still wins over package metadata.
    let mut explicit_config = IndexMap::new();
    explicit_config.insert(
        "explicit-provider".to_string(),
        ConfigProvider {
            name: Some("Explicit".to_string()),
            npm: Some("@ai-sdk/custom-sdk".to_string()),
            api: Some("https://api.example/v2".to_string()),
            models: Some(IndexMap::from_iter([(
                "custom-2".to_string(),
                provider::registry::ConfigModel::default(),
            )])),
            ..Default::default()
        },
    );
    let config = ConfigInput {
        provider: &explicit_config,
        ..Default::default()
    };
    let input = RegistryInput {
        catalog: &catalog,
        config,
        envs: &BTreeMap::new(),
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let providers = oc_provider::provider::registry::build_registry_with_npm_metadata(
        &input,
        &[],
        Some(&resolver),
    )
    .unwrap();
    assert_eq!(
        providers["explicit-provider"].models["custom-2"].api.url,
        "https://api.example/v2"
    );
}

#[test]
fn default_model_selection_parity() {
    use oc_provider::provider::default_model;

    let catalog = snapshot();
    // Build a small registry where two providers are env-connected.
    let mut envs = BTreeMap::new();
    envs.insert("OPENAI_API_KEY".to_string(), Some("sk-a".to_string()));
    envs.insert("ANTHROPIC_API_KEY".to_string(), Some("sk-b".to_string()));
    let input = RegistryInput {
        catalog: &catalog,
        config: ConfigInput::default(),
        envs: &envs,
        auths: &BTreeMap::new(),
        enable_experimental_models: false,
    };
    let providers = build_registry(&input).unwrap();

    // 1. Explicit cfg.model wins.
    let (p, m) = default_model(&providers, Some("openai/gpt-5.2-pro"), &[], &[]).unwrap();
    assert_eq!((p.as_str(), m.as_str()), ("openai", "gpt-5.2-pro"));

    // 2. Recent model.json hits when present in the registry.
    let (p, m) = default_model(
        &providers,
        None,
        &[],
        &[("anthropic".to_string(), "claude-sonnet-4-6".to_string())],
    )
    .unwrap();
    assert_eq!((p.as_str(), m.as_str()), ("anthropic", "claude-sonnet-4-6"));

    // 3. First provider in registry order otherwise.
    let first_connected = providers
        .iter()
        .find(|(_, p)| !p.models.is_empty())
        .map(|(id, _)| id.clone())
        .expect("at least one connected provider");
    let (p, m) = default_model(&providers, None, &[], &[]).unwrap();
    assert_eq!(
        p, first_connected,
        "should pick first registry-order provider"
    );
    assert!(!m.is_empty());

    // No configured providers and no matches -> NoProvidersError.
    let empty: IndexMap<String, oc_provider::provider::Info> = IndexMap::new();
    assert!(matches!(
        default_model(&empty, None, &[], &[]),
        Err(oc_provider::provider::DefaultModelError::NoProviders(_))
    ));
}
