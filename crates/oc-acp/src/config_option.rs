//! Session config option builders.
//!
//! From reference/packages/opencode/src/acp/config-option.ts.

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::sdk::ProviderInfo;
use crate::types::{SessionConfigOption, SessionConfigSelectOption};

/// The default effort variant name.
pub const DEFAULT_VARIANT_VALUE: &str = "default";

/// A model listed in a provider's `models` map.
#[derive(Debug, Clone)]
pub struct ConfigOptionModel {
    pub id: String,
    pub name: String,
    pub variants: Option<IndexMap<String, Map<String, Value>>>,
}

/// A provider fed to the config option builders.
#[derive(Debug, Clone)]
pub struct ConfigOptionProvider {
    pub id: String,
    pub name: String,
    pub models: IndexMap<String, ConfigOptionModel>,
}

/// A session mode option.
#[derive(Debug, Clone)]
pub struct ConfigOptionMode {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// A selected model plus optional effort variant.
#[derive(Debug, Clone)]
pub struct ModelSelection {
    pub model: ModelRef,
    pub variant: Option<String>,
}

/// `{ providerID, modelID }`.
#[derive(Debug, Clone)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

/// Adapt opencode `ProviderInfo` values to `ConfigOptionProvider`.
pub fn providers_from_info(providers: &[ProviderInfo]) -> Vec<ConfigOptionProvider> {
    providers
        .iter()
        .map(|provider| ConfigOptionProvider {
            id: provider.id.clone(),
            name: provider.name.clone(),
            models: provider
                .models
                .iter()
                .map(|(id, model)| {
                    (
                        id.clone(),
                        ConfigOptionModel {
                            id: model.id.clone(),
                            name: model.name.clone(),
                            variants: model.variants.clone(),
                        },
                    )
                })
                .collect(),
        })
        .collect()
}

/// `buildModelSelectOption` from reference/packages/opencode/src/acp/config-option.ts.
pub fn build_model_select_option(input: BuildModelSelectOptionInput) -> SessionConfigOption {
    SessionConfigOption {
        id: "model".into(),
        name: "Model".into(),
        description: None,
        category: Some("model".into()),
        r#type: "select".into(),
        current_value: format_current_model_id(FormatCurrentModelIdInput {
            model: &input.current_model,
            variant: input.current_variant,
            variants: &variants_for_model(&input.providers, &input.current_model),
            include_variant: input.include_variants,
        }),
        options: build_model_select_options(
            &input.providers,
            input.include_variants.unwrap_or(false),
        ),
    }
}

/// Input to [`build_model_select_option`].
pub struct BuildModelSelectOptionInput<'a> {
    pub providers: &'a [ConfigOptionProvider],
    pub current_model: &'a ModelRef,
    pub current_variant: Option<&'a str>,
    pub include_variants: Option<bool>,
}

/// `buildEffortSelectOption` from reference/packages/opencode/src/acp/config-option.ts.
pub fn build_effort_select_option(
    variants: &[String],
    current_variant: Option<&str>,
) -> Option<SessionConfigOption> {
    if variants.is_empty() {
        return None;
    }
    Some(SessionConfigOption {
        id: "effort".into(),
        name: "Effort".into(),
        description: Some("Available effort levels for this model".into()),
        category: Some("thought_level".into()),
        r#type: "select".into(),
        current_value: select_variant(current_variant, variants),
        options: variants
            .iter()
            .map(|variant| SessionConfigSelectOption {
                value: variant.clone(),
                name: format_variant_name(variant),
                description: None,
            })
            .collect(),
    })
}

/// `buildModeSelectOption` from reference/packages/opencode/src/acp/config-option.ts.
pub fn build_mode_select_option(
    modes: &[ConfigOptionMode],
    current_mode_id: &str,
) -> SessionConfigOption {
    SessionConfigOption {
        id: "mode".into(),
        name: "Session Mode".into(),
        description: None,
        category: Some("mode".into()),
        r#type: "select".into(),
        current_value: current_mode_id.to_string(),
        options: modes
            .iter()
            .map(|mode| SessionConfigSelectOption {
                value: mode.id.clone(),
                name: mode.name.clone(),
                description: mode.description.clone(),
            })
            .collect(),
    }
}

/// `buildConfigOptions` from reference/packages/opencode/src/acp/config-option.ts.
pub fn build_config_options(input: BuildConfigOptionsInput) -> Vec<SessionConfigOption> {
    let variants = variants_for_model(input.providers, &input.current_model);
    let effort = build_effort_select_option(&variants, input.current_variant);

    let mut options = vec![build_model_select_option(BuildModelSelectOptionInput {
        providers: input.providers,
        current_model: input.current_model,
        current_variant: input.current_variant,
        include_variants: input.include_model_variants,
    })];
    if let Some(effort) = effort {
        options.push(effort);
    }
    if let (Some(modes), Some(current_mode_id)) = (input.modes, input.current_mode_id) {
        options.push(build_mode_select_option(modes, current_mode_id));
    }
    options
}

/// Input to [`build_config_options`].
pub struct BuildConfigOptionsInput<'a> {
    pub providers: &'a [ConfigOptionProvider],
    pub current_model: &'a ModelRef,
    pub current_variant: Option<&'a str>,
    pub include_model_variants: Option<bool>,
    pub modes: Option<&'a [ConfigOptionMode]>,
    pub current_mode_id: Option<&'a str>,
}

/// `parseModelSelection` from reference/packages/opencode/src/acp/config-option.ts.
pub fn parse_model_selection(model_id: &str, providers: &[ConfigOptionProvider]) -> ModelSelection {
    if let Some(provider) = providers
        .iter()
        .find(|item| model_id.starts_with(&format!("{}/", item.id)))
    {
        let model_id_rest = &model_id[provider.id.len() + 1..];
        if provider.models.contains_key(model_id_rest) {
            return ModelSelection {
                model: ModelRef {
                    provider_id: provider.id.clone(),
                    model_id: model_id_rest.to_string(),
                },
                variant: None,
            };
        }
        if let Some(separator) = model_id_rest.rfind('/') {
            let base_model_id = &model_id_rest[..separator];
            let variant = &model_id_rest[separator + 1..];
            if let Some(model) = provider.models.get(base_model_id) {
                if let Some(variants) = &model.variants {
                    if variants.contains_key(variant) {
                        return ModelSelection {
                            model: ModelRef {
                                provider_id: provider.id.clone(),
                                model_id: base_model_id.to_string(),
                            },
                            variant: Some(variant.to_string()),
                        };
                    }
                }
            }
        }
        return ModelSelection {
            model: ModelRef {
                provider_id: provider.id.clone(),
                model_id: model_id_rest.to_string(),
            },
            variant: None,
        };
    }

    match model_id.find('/') {
        None => ModelSelection {
            model: ModelRef {
                provider_id: model_id.to_string(),
                model_id: String::new(),
            },
            variant: None,
        },
        Some(separator) => ModelSelection {
            model: ModelRef {
                provider_id: model_id[..separator].to_string(),
                model_id: model_id[separator + 1..].to_string(),
            },
            variant: None,
        },
    }
}

/// `formatCurrentModelId` from reference/packages/opencode/src/acp/config-option.ts.
pub fn format_current_model_id(input: FormatCurrentModelIdInput) -> String {
    let base = format!("{}/{}", input.model.provider_id, input.model.model_id);
    if !input.include_variant.unwrap_or(false) || input.variants.is_empty() {
        return base;
    }
    format!("{}/{}", base, select_variant(input.variant, input.variants))
}

/// Input to [`format_current_model_id`].
pub struct FormatCurrentModelIdInput<'a> {
    pub model: &'a ModelRef,
    pub variant: Option<&'a str>,
    pub variants: &'a [String],
    pub include_variant: Option<bool>,
}

/// `formatVariantName` from reference/packages/opencode/src/acp/config-option.ts.
pub fn format_variant_name(variant: &str) -> String {
    variant
        .split(['-', '_'])
        .map(|part| {
            if part.is_empty() {
                part.to_string()
            } else {
                let mut chars = part.chars();
                let first = chars.next().unwrap().to_uppercase().collect::<String>();
                format!("{first}{}", chars.as_str())
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_model_select_options(
    providers: &[ConfigOptionProvider],
    include_variants: bool,
) -> Vec<SessionConfigSelectOption> {
    let mut options = Vec::new();
    for provider in providers {
        let mut models: Vec<&ConfigOptionModel> = provider.models.values().collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        for model in models {
            let base = SessionConfigSelectOption {
                value: format!("{}/{}", provider.id, model.id),
                name: format!("{}/{}", provider.name, model.name),
                description: None,
            };
            if !include_variants {
                options.push(base);
                continue;
            }
            let Some(variants) = &model.variants else {
                options.push(base);
                continue;
            };
            options.push(base);
            for variant in variants.keys() {
                if variant == DEFAULT_VARIANT_VALUE {
                    continue;
                }
                options.push(SessionConfigSelectOption {
                    value: format!("{}/{}/{}", provider.id, model.id, variant),
                    name: format!(
                        "{}/{} ({})",
                        provider.name,
                        model.name,
                        format_variant_name(variant)
                    ),
                    description: None,
                });
            }
        }
    }
    options
}

fn variants_for_model(providers: &[ConfigOptionProvider], model: &ModelRef) -> Vec<String> {
    providers
        .iter()
        .find(|provider| provider.id == model.provider_id)
        .and_then(|provider| provider.models.get(&model.model_id))
        .and_then(|model| model.variants.as_ref())
        .map(|variants| variants.keys().cloned().collect())
        .unwrap_or_default()
}

fn select_variant(variant: Option<&str>, variants: &[String]) -> String {
    if let Some(variant) = variant {
        if variants.iter().any(|item| item == variant) {
            return variant.to_string();
        }
    }
    if variants.iter().any(|item| item == DEFAULT_VARIANT_VALUE) {
        return DEFAULT_VARIANT_VALUE.to_string();
    }
    variants.first().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, models: &[(&str, &str, Option<&[&str]>)]) -> ConfigOptionProvider {
        ConfigOptionProvider {
            id: id.into(),
            name: id.into(),
            models: models
                .iter()
                .map(|(model_id, name, variants)| {
                    let variants = variants.map(|variants| {
                        variants
                            .iter()
                            .map(|variant| (variant.to_string(), Map::new()))
                            .collect()
                    });
                    (
                        model_id.to_string(),
                        ConfigOptionModel {
                            id: model_id.to_string(),
                            name: name.to_string(),
                            variants,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn parse_simple_model() {
        let providers = vec![provider(
            "anthropic",
            &[("claude-sonnet-4", "Claude", None)],
        )];
        let selection = parse_model_selection("anthropic/claude-sonnet-4", &providers);
        assert_eq!(selection.model.provider_id, "anthropic");
        assert_eq!(selection.model.model_id, "claude-sonnet-4");
        assert_eq!(selection.variant, None);
    }

    #[test]
    fn parse_variant_model() {
        let providers = vec![provider(
            "anthropic",
            &[("claude-sonnet-4", "Claude", Some(&["high", "default"]))],
        )];
        let selection = parse_model_selection("anthropic/claude-sonnet-4/high", &providers);
        assert_eq!(selection.model.model_id, "claude-sonnet-4");
        assert_eq!(selection.variant.as_deref(), Some("high"));
    }

    #[test]
    fn parse_unlisted_model() {
        let providers = vec![provider("anthropic", &[])];
        let selection = parse_model_selection("anthropic/claude-opus-5", &providers);
        assert_eq!(selection.model.provider_id, "anthropic");
        assert_eq!(selection.model.model_id, "claude-opus-5");
    }

    #[test]
    fn parse_without_provider() {
        let providers: Vec<ConfigOptionProvider> = vec![];
        let selection = parse_model_selection("provider/model", &providers);
        assert_eq!(selection.model.provider_id, "provider");
        assert_eq!(selection.model.model_id, "model");
    }

    #[test]
    fn format_variant_name_test() {
        assert_eq!(format_variant_name("low"), "Low");
        assert_eq!(format_variant_name("high-effort"), "High Effort");
        assert_eq!(format_variant_name("super_fast"), "Super Fast");
    }

    #[test]
    fn build_config_options_shape() {
        let providers = vec![provider(
            "anthropic",
            &[(
                "claude-sonnet-4",
                "Claude Sonnet",
                Some(&["default", "high"]),
            )],
        )];
        let modes = vec![ConfigOptionMode {
            id: "build".into(),
            name: "build".into(),
            description: None,
        }];
        let options = build_config_options(BuildConfigOptionsInput {
            providers: &providers,
            current_model: &ModelRef {
                provider_id: "anthropic".into(),
                model_id: "claude-sonnet-4".into(),
            },
            current_variant: None,
            include_model_variants: Some(true),
            modes: Some(&modes),
            current_mode_id: Some("build"),
        });
        assert_eq!(
            serde_json::to_value(&options).unwrap(),
            serde_json::json!([
                {
                    "id": "model",
                    "name": "Model",
                    "category": "model",
                    "type": "select",
                    "currentValue": "anthropic/claude-sonnet-4/default",
                    "options": [
                        { "value": "anthropic/claude-sonnet-4", "name": "anthropic/Claude Sonnet" },
                        { "value": "anthropic/claude-sonnet-4/high", "name": "anthropic/Claude Sonnet (High)" }
                    ]
                },
                {
                    "id": "effort",
                    "name": "Effort",
                    "description": "Available effort levels for this model",
                    "category": "thought_level",
                    "type": "select",
                    "currentValue": "default",
                    "options": [
                        { "value": "default", "name": "Default" },
                        { "value": "high", "name": "High" }
                    ]
                },
                {
                    "id": "mode",
                    "name": "Session Mode",
                    "category": "mode",
                    "type": "select",
                    "currentValue": "build",
                    "options": [
                        { "value": "build", "name": "build" }
                    ]
                }
            ])
        );
    }
}
