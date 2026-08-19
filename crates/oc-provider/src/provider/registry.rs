//! Provider registry construction.
//!
//! From the layer effect of reference/packages/opencode/src/provider/provider.ts.
//!
//! The reference builds the provider registry inside an Effect layer with
//! plugin hooks and a bundled SDK loader. The Rust port extracts the registry
//! construction into a pure [`build_registry`] function over injected inputs
//! (models.dev catalog, config, env, auth), which is what the CLI and server
//! surfaces need. The executable plugin callback and SDK resolution
//! (`resolveSDK`/`getLanguage`) remain LLM-layer concerns not present here.
//!
//! A typed model-hook result seam is provided by
//! [`build_registry_with_model_hooks`]. It accepts already materialized model
//! data so callers can preserve the reference ordering (hook result, then
//! config overrides) without pretending that a JavaScript callback is
//! executable here. The server documents and owns the remaining runtime
//! boundary.

//! TODO(integration): run plugin `auth.loader` option patches before/after the
//! config merge; wire `gitlab` workflow-model discovery (network) in oc-llm.

use std::collections::{BTreeMap, HashSet};

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::auth::Info as AuthInfo;
use crate::models_dev;
use crate::provider::transform::{self, VariantMap};
use crate::provider::{
    self, from_models_dev_provider, merge_deep, merge_provider, ApiInfo, Capabilities, Cost,
    ExperimentalOver200K, Info, InterleavedField, Limit, Modalities, Model, ModelStatus, Source,
};

/// `ConfigProviderV1.Info` from reference/packages/core/src/v1/config/provider.ts.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProvider {
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub env: Option<Vec<String>>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub whitelist: Option<Vec<String>>,
    #[serde(default)]
    pub blacklist: Option<Vec<String>>,
    #[serde(default)]
    pub options: Option<Map<String, Value>>,
    #[serde(default)]
    pub models: Option<IndexMap<String, ConfigModel>>,
}

/// `ConfigProviderV1.Model` from `config/provider.ts`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigModel {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub attachment: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub interleaved: Option<Interleaved>,
    #[serde(default)]
    pub cost: Option<ConfigCost>,
    #[serde(default)]
    pub limit: Option<ConfigLimit>,
    #[serde(default)]
    pub modalities: Option<ConfigModalities>,
    #[serde(default)]
    pub experimental: Option<bool>,
    #[serde(default)]
    pub status: Option<ModelStatus>,
    #[serde(default)]
    pub provider: Option<ConfigProviderRef>,
    #[serde(default)]
    pub options: Option<Map<String, Value>>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub variants: Option<IndexMap<String, ConfigVariant>>,
}

/// Config `interleaved`: `boolean | string | { field }`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Interleaved {
    Bool(bool),
    Field(String),
    Struct { field: String },
}

/// `ConfigProviderV1.Model.cost`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigCost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    #[serde(default)]
    pub context_over_200k: Option<ConfigContextOver200K>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigContextOver200K {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// `ConfigProviderV1.Model.limit`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigLimit {
    #[serde(default)]
    pub context: Option<f64>,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
}

/// `ConfigProviderV1.Model.modalities`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigModalities {
    #[serde(default)]
    pub input: Option<Vec<models_dev::Modality>>,
    #[serde(default)]
    pub output: Option<Vec<models_dev::Modality>>,
}

/// `ConfigProviderV1.Model.provider`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProviderRef {
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
}

/// A config variant: `{ disabled?, ...rest }`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigVariant {
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl ConfigVariant {
    fn as_value(&self) -> Map<String, Value> {
        let mut map = self.rest.clone();
        if let Some(disabled) = self.disabled {
            map.insert("disabled".to_string(), Value::from(disabled));
        }
        map
    }
}

/// `ConfigV1.Info` subset consumed by the registry.
#[derive(Debug, Clone)]
pub struct ConfigInput<'a> {
    pub provider: &'a IndexMap<String, ConfigProvider>,
    pub disabled_providers: Option<&'a [String]>,
    pub enabled_providers: Option<&'a [String]>,
}

impl<'a> Default for ConfigInput<'a> {
    fn default() -> Self {
        static EMPTY: std::sync::OnceLock<IndexMap<String, ConfigProvider>> =
            std::sync::OnceLock::new();
        ConfigInput {
            provider: EMPTY.get_or_init(IndexMap::new),
            disabled_providers: None,
            enabled_providers: None,
        }
    }
}

/// Injectable npm-package metadata fetch seam.
///
/// The reference resolves a custom provider's endpoint implicitly by loading
/// the named npm SDK package, whose bundled defaults carry the provider's
/// base URL. The native registry cannot load JS SDKs, so this seam surfaces
/// that package metadata: given an npm package name, it returns the SDK's
/// default API base URL to use as the fallback for models whose config and
/// models.dev catalog entries do not supply an explicit `api` URL.
///
/// Implementations are injectable so tests can serve fixture metadata instead
/// of touching the network. It is intentionally optional: when `None`, no npm
/// metadata fallback is applied and behaviour stays identical to the
/// reference config-only path.
pub trait NpmMetadata {
    /// Resolve the default API base URL advertised by `npm`'s package
    /// metadata, if any.
    fn provider_base_url(&self, npm: &str) -> Option<String>;
}

/// Inputs to [`build_registry`].
#[derive(Debug, Clone)]
pub struct RegistryInput<'a> {
    /// The models.dev catalog (`Record<ProviderID, Provider>`).
    pub catalog: &'a IndexMap<String, models_dev::Provider>,
    /// The parsed `opencode.json` provider section and allowlists.
    pub config: ConfigInput<'a>,
    /// `Env.all()` snapshot.
    pub envs: &'a BTreeMap<String, Option<String>>,
    /// `Auth.all()` snapshot.
    pub auths: &'a BTreeMap<String, AuthInfo>,
    /// `RuntimeFlags.enableExperimentalModels`.
    pub enable_experimental_models: bool,
}

/// Materialized result of a plugin `Hooks.provider.models` callback.
///
/// The reference callback returns a complete model map for an existing
/// provider. Keeping this seam typed makes replacement and subsequent config
/// merging deterministic while leaving callback execution to the host layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderModelHookRegistration {
    pub provider_id: String,
    pub models: IndexMap<String, Model>,
}

impl ProviderModelHookRegistration {
    pub fn new(provider_id: impl Into<String>, models: IndexMap<String, Model>) -> Self {
        Self {
            provider_id: provider_id.into(),
            models,
        }
    }
}

/// The result of a custom provider loader (options + autoload).
pub(crate) struct LoaderResult {
    pub autoload: bool,
    pub options: Map<String, Value>,
}

fn env_get<'a>(envs: &'a BTreeMap<String, Option<String>>, key: &str) -> Option<&'a str> {
    // JavaScript's `Boolean(env[key])` is used by the reference loader, so an
    // explicitly configured empty string is not a credential. Keep whitespace
    // intact: a non-empty value is still truthy in the reference runtime.
    envs.get(key)
        .and_then(|v| v.as_deref())
        .filter(|value| !value.is_empty())
}

/// The provider-specific loaders from `custom()` in `provider.ts`.
///
/// Only the registry-relevant parts are ported: `autoload` and `options`.
/// `getModel`/`vars` loaders (SDK selection, region vars) are LLM-layer
/// concerns.
///
/// TODO(integration): port `getModel`/`vars`/`discoverModels` loaders into
/// `oc-llm`; the Amazon Bedrock credential chain and Vertex ADC token fetch
/// (`google-auth-library`) are native-runtime concerns.
fn custom_loader(
    provider_id: &str,
    provider: &mut Info,
    input: &RegistryInput,
) -> Result<Option<LoaderResult>, anyhow::Error> {
    let envs = input.envs;
    let config_provider = input.config.provider.get(provider_id);
    let auth = input.auths.get(provider_id);

    let loader = match provider_id {
        "anthropic" => LoaderResult {
            autoload: false,
            options: json!({
                "headers": {
                    "anthropic-beta": "interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "opencode" => {
            let has_key = provider.env.iter().any(|item| env_get(envs, item).is_some());
            let has_auth = auth.is_some();
            let config_api_key = config_provider
                .and_then(|p| p.options.as_ref())
                .and_then(|o| o.get("apiKey"))
                .and_then(|v| v.as_str())
                .is_some_and(|k| !k.is_empty());
            let ok = has_key || has_auth || config_api_key;
            if !ok {
                provider.models.retain(|_, model| model.cost.input != 0.0);
            }
            LoaderResult {
                autoload: !provider.models.is_empty(),
                options: if ok {
                    Map::new()
                } else {
                    json!({ "apiKey": "public" }).as_object().unwrap().clone()
                },
            }
        }
        "openai" => LoaderResult {
            autoload: false,
            options: json!({ "headerTimeout": 300_000 }).as_object().unwrap().clone(),
        },
        "meta" | "xai" | "github-copilot" => LoaderResult {
            autoload: false,
            options: Map::new(),
        },
        "azure" => {
            let resource = [
                provider.options.get("resourceName").and_then(|v| v.as_str()),
                match auth {
                    Some(AuthInfo::Api(api)) => api
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("resourceName"))
                        .map(String::as_str),
                    _ => None,
                },
                env_get(envs, "AZURE_RESOURCE_NAME"),
            ]
            .into_iter()
            .flatten()
            .find(|name| !name.trim().is_empty());

            if resource.is_none() && provider.options.get("baseURL").is_none() {
                return Ok(None);
            }
            let mut options = Map::new();
            if let Some(resource) = resource {
                options.insert("resourceName".to_string(), Value::from(resource));
            }
            LoaderResult {
                autoload: false,
                options,
            }
        }
        "azure-cognitive-services" => {
            let resource_name = env_get(envs, "AZURE_COGNITIVE_SERVICES_RESOURCE_NAME");
            let mut options = Map::new();
            if let Some(resource_name) = resource_name {
                let use_deployment = provider
                    .options
                    .get("useDeploymentBasedUrls")
                    .is_some_and(|v| v.as_bool() == Some(true));
                let suffix = if use_deployment { "" } else { "/v1" };
                options.insert(
                    "baseURL".to_string(),
                    Value::from(format!(
                        "https://{}.cognitiveservices.azure.com/openai{}",
                        resource_name, suffix
                    )),
                );
            }
            LoaderResult {
                autoload: false,
                options,
            }
        }
        "amazon-bedrock" => {
            let config_region = config_provider
                .and_then(|p| p.options.as_ref())
                .and_then(|o| o.get("region"))
                .and_then(|v| v.as_str());
            let env_region = env_get(envs, "AWS_REGION");
            let default_region = config_region.or(env_region).unwrap_or("us-east-1");
            let config_profile = config_provider
                .and_then(|p| p.options.as_ref())
                .and_then(|o| o.get("profile"))
                .and_then(|v| v.as_str());
            let env_profile = env_get(envs, "AWS_PROFILE");
            let profile = config_profile.or(env_profile);
            let aws_access_key_id = env_get(envs, "AWS_ACCESS_KEY_ID");
            let config_api_key = config_provider
                .and_then(|p| p.options.as_ref())
                .and_then(|o| o.get("apiKey"))
                .and_then(|v| v.as_str());
            let aws_bearer_token = env_get(envs, "AWS_BEARER_TOKEN_BEDROCK")
                .map(str::to_string)
                .or_else(|| match auth {
                    Some(AuthInfo::Api(api)) => Some(api.key.clone()),
                    _ => None,
                });
            let aws_web_identity_token_file = env_get(envs, "AWS_WEB_IDENTITY_TOKEN_FILE");
            let container_creds = envs
                .get("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI")
                .or_else(|| envs.get("AWS_CONTAINER_CREDENTIALS_FULL_URI"))
                .is_some_and(|v| v.is_some());

            if profile.is_none()
                && aws_access_key_id.is_none()
                && aws_bearer_token.is_none()
                && config_api_key.is_none()
                && aws_web_identity_token_file.is_none()
                && !container_creds
            {
                return Ok(None);
            }

            let mut options = Map::new();
            options.insert("region".to_string(), Value::from(default_region));
            let endpoint = config_provider
                .and_then(|p| p.options.as_ref())
                .and_then(|o| o.get("endpoint"))
                .or_else(|| {
                    config_provider
                        .and_then(|p| p.options.as_ref())
                        .and_then(|o| o.get("baseURL"))
                })
                .cloned();
            if let Some(endpoint) = endpoint {
                options.insert("baseURL".to_string(), endpoint);
            }
            LoaderResult {
                autoload: true,
                options,
            }
        }
        "llmgateway" => LoaderResult {
            autoload: false,
            options: json!({
                "headers": {
                    "HTTP-Referer": "https://opencode.ai/",
                    "X-Title": "opencode",
                    "X-Source": "opencode",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "openrouter" => LoaderResult {
            autoload: false,
            options: json!({
                "headers": {
                    "HTTP-Referer": "https://opencode.ai/",
                    "X-Title": "opencode",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "nvidia" => LoaderResult {
            autoload: provider.source == Source::Config,
            options: json!({
                "headers": {
                    "HTTP-Referer": "https://opencode.ai/",
                    "X-Title": "opencode",
                    "X-BILLING-INVOKE-ORIGIN": "OpenCode",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "vercel" => LoaderResult {
            autoload: false,
            options: json!({
                "headers": {
                    "http-referer": "https://opencode.ai/",
                    "x-title": "opencode",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "google-vertex" => {
            let project = provider
                .options
                .get("project")
                .and_then(|v| v.as_str())
                .or_else(|| env_get(envs, "GOOGLE_VERTEX_PROJECT"))
                .or_else(|| env_get(envs, "GOOGLE_CLOUD_PROJECT"))
                .or_else(|| env_get(envs, "GCP_PROJECT"))
                .or_else(|| env_get(envs, "GCLOUD_PROJECT"))
                .map(str::to_string);
            let location = provider
                .options
                .get("location")
                .and_then(|v| v.as_str())
                .or_else(|| env_get(envs, "GOOGLE_VERTEX_LOCATION"))
                .or_else(|| env_get(envs, "GOOGLE_CLOUD_LOCATION"))
                .or_else(|| env_get(envs, "VERTEX_LOCATION"))
                .unwrap_or("us-central1")
                .to_string();
            let autoload = project.is_some();
            let mut options = Map::new();
            if let Some(project) = project {
                options.insert("project".to_string(), Value::from(project));
            }
            options.insert("location".to_string(), Value::from(location));
            LoaderResult {
                autoload,
                options,
            }
        }
        "google-vertex-anthropic" => {
            let project = env_get(envs, "GOOGLE_CLOUD_PROJECT")
                .or_else(|| env_get(envs, "GCP_PROJECT"))
                .or_else(|| env_get(envs, "GCLOUD_PROJECT"))
                .map(str::to_string);
            let location = env_get(envs, "GOOGLE_CLOUD_LOCATION")
                .or_else(|| env_get(envs, "VERTEX_LOCATION"))
                .unwrap_or("global");
            let autoload = project.is_some();
            let mut options = Map::new();
            if let Some(project) = project {
                options.insert("project".to_string(), Value::from(project));
            }
            options.insert("location".to_string(), Value::from(location));
            if let Some(base_url) = google_vertex_anthropic_base_url(
                env_get(envs, "GOOGLE_CLOUD_PROJECT").or(env_get(envs, "GCP_PROJECT")).or(env_get(envs, "GCLOUD_PROJECT")),
                env_get(envs, "GOOGLE_CLOUD_LOCATION").or(env_get(envs, "VERTEX_LOCATION")),
            ) {
                options.insert("baseURL".to_string(), Value::from(base_url));
            }
            LoaderResult {
                autoload,
                options,
            }
        }
        "sap-ai-core" => {
            let env_service_key = env_get(envs, "AICORE_SERVICE_KEY").map(str::to_string).or_else(|| match auth {
                Some(AuthInfo::Api(api)) => Some(api.key.clone()),
                _ => None,
            });
            let deployment_id = env_get(envs, "AICORE_DEPLOYMENT_ID").map(str::to_string);
            let resource_group = env_get(envs, "AICORE_RESOURCE_GROUP").map(str::to_string);
            let mut options = Map::new();
            if let Some(deployment_id) = deployment_id {
                options.insert("deploymentId".to_string(), Value::from(deployment_id));
            }
            if let Some(resource_group) = resource_group {
                options.insert("resourceGroup".to_string(), Value::from(resource_group));
            }
            LoaderResult {
                autoload: env_service_key.is_some(),
                options,
            }
        }
        "zenmux" => LoaderResult {
            autoload: false,
            options: json!({
                "headers": {
                    "HTTP-Referer": "https://opencode.ai/",
                    "X-Title": "opencode",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "gitlab" => {
            let api_key = match auth {
                Some(AuthInfo::Oauth(oauth)) => Some(oauth.access.clone()),
                Some(AuthInfo::Api(api)) => Some(api.key.clone()),
                _ => None,
            };
            let token = api_key.or_else(|| env_get(envs, "GITLAB_TOKEN").map(str::to_string));
            let instance_url = env_get(envs, "GITLAB_INSTANCE_URL").unwrap_or("https://gitlab.com").to_string();
            let mut options = Map::new();
            options.insert("instanceUrl".to_string(), Value::from(instance_url));
            if let Some(token) = token {
                options.insert("apiKey".to_string(), Value::from(token));
            }
            // TODO(integration): aiGatewayHeaders carries a version-stamped
            // User-Agent and `featureFlags`/`discoverWorkflowModels` need the
            // gitlab-ai-provider runtime.
            LoaderResult {
                autoload: options.get("apiKey").is_some(),
                options,
            }
        }
        "cloudflare-workers-ai" => {
            if provider.options.get("baseURL").is_some() {
                return Ok(None);
            }
            let account_id = env_get(envs, "CLOUDFLARE_ACCOUNT_ID").map(str::to_string).or_else(|| match auth {
                Some(AuthInfo::Api(api)) => api.metadata.as_ref().and_then(|m| m.get("accountId")).cloned(),
                _ => None,
            });
            let Some(account_id) = account_id else {
                return Ok(None);
            };
            let api_key = env_get(envs, "CLOUDFLARE_API_KEY").map(str::to_string).or_else(|| match auth {
                Some(AuthInfo::Api(api)) => Some(api.key.clone()),
                _ => None,
            });
            let mut options = Map::new();
            if let Some(api_key) = api_key {
                options.insert("apiKey".to_string(), Value::from(api_key));
            }
            let _ = account_id;
            LoaderResult {
                autoload: options.get("apiKey").is_some(),
                options,
            }
        }
        "cloudflare-ai-gateway" => {
            if provider.options.get("baseURL").is_some() {
                return Ok(None);
            }
            let account_id = env_get(envs, "CLOUDFLARE_ACCOUNT_ID").map(str::to_string).or_else(|| match auth {
                Some(AuthInfo::Api(api)) => api.metadata.as_ref().and_then(|m| m.get("accountId")).cloned(),
                _ => None,
            });
            let gateway = env_get(envs, "CLOUDFLARE_GATEWAY_ID").map(str::to_string).or_else(|| match auth {
                Some(AuthInfo::Api(api)) => api.metadata.as_ref().and_then(|m| m.get("gatewayId")).cloned(),
                _ => None,
            });
            if account_id.is_none() || gateway.is_none() {
                return Ok(None);
            }
            let api_token = env_get(envs, "CLOUDFLARE_API_TOKEN")
                .or_else(|| env_get(envs, "CF_AIG_TOKEN"))
                .map(str::to_string)
                .or_else(|| match auth {
                    Some(AuthInfo::Api(api)) => Some(api.key.clone()),
                    _ => None,
                });
            if api_token.is_none() {
                return Err(anyhow::anyhow!(
                    "CLOUDFLARE_API_TOKEN (or CF_AIG_TOKEN) is required for Cloudflare AI Gateway. Set it via environment variable or run `opencode auth cloudflare-ai-gateway`."
                ));
            }
            let options = Map::new();
            // TODO(integration): `createAiGateway`/`createUnified` and the
            // metadata/cacheTtl/cacheKey/skipCache/collectLog options are
            // wired in oc-llm.
            LoaderResult {
                autoload: true,
                options,
            }
        }
        "cerebras" => LoaderResult {
            autoload: false,
            options: json!({
                "headers": {
                    "X-Cerebras-3rd-Party-Integration": "opencode",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "kilo" => LoaderResult {
            autoload: false,
            options: json!({
                "headers": {
                    "HTTP-Referer": "https://opencode.ai/",
                    "X-Title": "opencode",
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        },
        "snowflake-cortex" => {
            let account = env_get(envs, "SNOWFLAKE_ACCOUNT").map(str::to_string).or_else(|| match auth {
                Some(AuthInfo::Api(api)) => api.metadata.as_ref().and_then(|m| m.get("account")).cloned(),
                _ => None,
            });
            let account = account.or_else(|| match auth {
                Some(AuthInfo::Oauth(oauth)) => oauth.account_id.clone(),
                _ => None,
            });
            let account = account.or_else(|| provider.options.get("account").and_then(|v| v.as_str()).map(str::to_string));

            let env_token = env_get(envs, "SNOWFLAKE_CORTEX_TOKEN")
                .or_else(|| env_get(envs, "SNOWFLAKE_CORTEX_PAT"))
                .map(str::to_string);
            let api_key_token = match auth {
                Some(AuthInfo::Api(api)) => Some(api.key.clone()),
                _ => None,
            };
            let oauth_token = match auth {
                Some(AuthInfo::Oauth(oauth)) => Some(oauth.access.clone()),
                _ => None,
            };
            let config_token = provider
                .options
                .get("token")
                .or_else(|| provider.options.get("apiKey"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let token = env_token
                .or(api_key_token)
                .or(oauth_token)
                .or(config_token);

            let Some(account) = account else {
                return Ok(None);
            };
            let Some(token) = token else {
                return Ok(None);
            };
            let mut options = Map::new();
            options.insert(
                "baseURL".to_string(),
                Value::from(format!("https://{}.snowflakecomputing.com/api/v2/cortex/v1", account)),
            );
            options.insert("apiKey".to_string(), Value::from(token));
            LoaderResult {
                autoload: provider.source == Source::Config,
                options,
            }
        }
        _ if provider.source == Source::Config
            && (!provider.models.is_empty()
                || provider.options.contains_key("apiKey")
                || provider.options.contains_key("baseURL")
                || provider.options.contains_key("baseUrl")) =>
        {
            // A config-defined provider with explicit models or endpoint
            // options is usable without a built-in provider loader. Keep it
            // in the active registry so OpenAI-compatible adapters can route
            // it through the configured base URL.
            LoaderResult {
                autoload: true,
                options: Map::new(),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(loader))
}

fn google_vertex_anthropic_base_url(
    project: Option<&str>,
    location: Option<&str>,
) -> Option<String> {
    let project = project?;
    if location != Some("eu") && location != Some("us") {
        return None;
    }
    Some(format!(
        "https://aiplatform.{}.rep.googleapis.com/v1/projects/{}/locations/{}/publishers/anthropic/models",
        location.unwrap(),
        project,
        location.unwrap()
    ))
}

/// Merges a config `model` into `parsed` (the provider under construction).
///
/// From the config-provider loop of the layer effect in `provider.ts`.
fn merge_config_model(
    parsed: &mut Info,
    model_id: &str,
    model: &ConfigModel,
    provider: Option<&ConfigProvider>,
    models_dev: Option<&models_dev::Provider>,
    npm_metadata: Option<&dyn NpmMetadata>,
) {
    let lookup = model.id.as_deref().unwrap_or(model_id);
    let existing_model = parsed.models.get(lookup).cloned();
    let api_id = model
        .id
        .clone()
        .or_else(|| existing_model.as_ref().map(|m| m.api.id.clone()))
        .unwrap_or_else(|| model_id.to_string());
    let api_npm = model
        .provider
        .as_ref()
        .and_then(|p| p.npm.clone())
        .or_else(|| provider.and_then(|p| p.npm.clone()))
        .or_else(|| existing_model.as_ref().map(|m| m.api.npm.clone()))
        .or_else(|| models_dev.and_then(|p| p.npm.clone()))
        .unwrap_or_else(|| "@ai-sdk/openai-compatible".to_string());
    let name = if let Some(name) = &model.name {
        name.clone()
    } else if let Some(id) = &model.id {
        if id != model_id {
            model_id.to_string()
        } else {
            existing_model
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| model_id.to_string())
        }
    } else {
        existing_model
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| model_id.to_string())
    };

    let modalities_input = model
        .modalities
        .as_ref()
        .and_then(|m| m.input.as_ref())
        .map(|list| list.contains(&models_dev::Modality::Text));
    let modalities_output = model
        .modalities
        .as_ref()
        .and_then(|m| m.output.as_ref())
        .map(|list| list.contains(&models_dev::Modality::Text));
    let _ = (modalities_input, modalities_output);

    let interleaved_config = match &model.interleaved {
        Some(Interleaved::Bool(b)) => Some(InterleavedField::Bool(*b)),
        Some(Interleaved::Field(f)) => Some(InterleavedField::Field { field: f.clone() }),
        Some(Interleaved::Struct { field }) => Some(InterleavedField::Field {
            field: field.clone(),
        }),
        None => None,
    };
    let interleaved_default = if existing_model.is_none()
        && api_npm == "@ai-sdk/openai-compatible"
        && api_id.contains("deepseek")
    {
        Some(InterleavedField::Field {
            field: "reasoning_content".to_string(),
        })
    } else {
        None
    };
    let interleaved = interleaved_config
        .or_else(|| {
            existing_model
                .as_ref()
                .map(|m| m.capabilities.interleaved.clone())
        })
        .or(interleaved_default)
        .unwrap_or(InterleavedField::Bool(false));

    let existing_caps = existing_model.as_ref().map(|m| m.capabilities.clone());
    let input_modalities = |modality: models_dev::Modality, fallback: bool| -> bool {
        model
            .modalities
            .as_ref()
            .and_then(|m| m.input.as_ref())
            .map(|list| list.contains(&modality))
            .or_else(|| existing_caps.as_ref().map(|c| c.input.get(modality)))
            .unwrap_or(fallback)
    };
    let output_modalities = |modality: models_dev::Modality, fallback: bool| -> bool {
        model
            .modalities
            .as_ref()
            .and_then(|m| m.output.as_ref())
            .map(|list| list.contains(&modality))
            .or_else(|| existing_caps.as_ref().map(|c| c.output.get(modality)))
            .unwrap_or(fallback)
    };

    let parsed_model = Model {
        id: model_id.to_string(),
        api: ApiInfo {
            id: api_id.clone(),
            npm: api_npm.clone(),
            url: model
                .provider
                .as_ref()
                .and_then(|p| p.api.clone())
                .or_else(|| provider.and_then(|p| p.api.clone()))
                .or_else(|| existing_model.as_ref().map(|m| m.api.url.clone()))
                .or_else(|| models_dev.and_then(|p| p.api.clone()))
                .or_else(|| {
                    // npm-package metadata fallback: a config-declared provider
                    // that names an npm SDK but supplies no explicit `api` URL
                    // (and is absent from the models.dev catalog) resolves its
                    // base URL from the package metadata seam.
                    npm_metadata.and_then(|resolver| resolver.provider_base_url(&api_npm))
                })
                .unwrap_or_default(),
        },
        status: model
            .status
            .or_else(|| existing_model.as_ref().map(|m| m.status))
            .unwrap_or(ModelStatus::Active),
        name,
        provider_id: parsed.id.clone(),
        capabilities: Capabilities {
            temperature: model
                .temperature
                .or_else(|| existing_caps.as_ref().map(|c| c.temperature))
                .unwrap_or(false),
            reasoning: model
                .reasoning
                .or_else(|| existing_caps.as_ref().map(|c| c.reasoning))
                .unwrap_or(false),
            attachment: model
                .attachment
                .or_else(|| existing_caps.as_ref().map(|c| c.attachment))
                .unwrap_or(false),
            toolcall: model
                .tool_call
                .or_else(|| existing_caps.as_ref().map(|c| c.toolcall))
                .unwrap_or(true),
            input: Modalities {
                text: input_modalities(models_dev::Modality::Text, true),
                audio: input_modalities(models_dev::Modality::Audio, false),
                image: input_modalities(models_dev::Modality::Image, false),
                video: input_modalities(models_dev::Modality::Video, false),
                pdf: input_modalities(models_dev::Modality::Pdf, false),
            },
            output: Modalities {
                text: output_modalities(models_dev::Modality::Text, true),
                audio: output_modalities(models_dev::Modality::Audio, false),
                image: output_modalities(models_dev::Modality::Image, false),
                video: output_modalities(models_dev::Modality::Video, false),
                pdf: output_modalities(models_dev::Modality::Pdf, false),
            },
            interleaved,
        },
        cost: {
            let existing_cost = existing_model.as_ref().map(|m| m.cost.clone());
            let mut cost = Cost {
                input: model
                    .cost
                    .as_ref()
                    .and_then(|c| c.input)
                    .or_else(|| existing_cost.as_ref().map(|c| c.input))
                    .unwrap_or(0.0),
                output: model
                    .cost
                    .as_ref()
                    .and_then(|c| c.output)
                    .or_else(|| existing_cost.as_ref().map(|c| c.output))
                    .unwrap_or(0.0),
                cache: provider::CacheCost {
                    read: model
                        .cost
                        .as_ref()
                        .and_then(|c| c.cache_read)
                        .or_else(|| existing_cost.as_ref().map(|c| c.cache.read))
                        .unwrap_or(0.0),
                    write: model
                        .cost
                        .as_ref()
                        .and_then(|c| c.cache_write)
                        .or_else(|| existing_cost.as_ref().map(|c| c.cache.write))
                        .unwrap_or(0.0),
                },
                tiers: existing_cost.as_ref().and_then(|c| c.tiers.clone()),
                experimental_over_200k: None,
            };
            if let Some(over) = &model
                .cost
                .as_ref()
                .and_then(|c| c.context_over_200k.clone())
            {
                cost.experimental_over_200k = Some(ExperimentalOver200K {
                    input: over.input.unwrap_or(0.0),
                    output: over.output.unwrap_or(0.0),
                    cache: provider::CacheCost {
                        read: over.cache_read.unwrap_or(0.0),
                        write: over.cache_write.unwrap_or(0.0),
                    },
                });
            }
            cost
        },
        options: merge_deep(
            Value::Object(
                existing_model
                    .as_ref()
                    .map(|m| m.options.clone())
                    .unwrap_or_default(),
            ),
            Value::Object(model.options.clone().unwrap_or_default()),
        )
        .as_object()
        .unwrap()
        .clone(),
        limit: Limit {
            context: model
                .limit
                .as_ref()
                .and_then(|l| l.context)
                .or_else(|| existing_model.as_ref().map(|m| m.limit.context))
                .unwrap_or(0.0),
            input: model
                .limit
                .as_ref()
                .and_then(|l| l.input)
                .or_else(|| existing_model.as_ref().and_then(|m| m.limit.input)),
            output: model
                .limit
                .as_ref()
                .and_then(|l| l.output)
                .or_else(|| existing_model.as_ref().map(|m| m.limit.output))
                .unwrap_or(0.0),
        },
        headers: {
            let mut headers = existing_model
                .as_ref()
                .map(|m| m.headers.clone())
                .unwrap_or_default();
            if let Some(config_headers) = &model.headers {
                headers.extend(config_headers.clone());
            }
            headers
        },
        family: model
            .family
            .clone()
            .or_else(|| existing_model.as_ref().and_then(|m| m.family.clone())),
        release_date: model
            .release_date
            .clone()
            .or_else(|| existing_model.as_ref().map(|m| m.release_date.clone()))
            .unwrap_or_default(),
        variants: VariantMap::new(),
    };

    let mut parsed_model = parsed_model;
    let variants = match &existing_model {
        Some(existing) if existing.api.npm == parsed_model.api.npm => {
            if existing.variants.is_empty() {
                transform::variants(&parsed_model)
            } else {
                existing.variants.clone()
            }
        }
        _ => transform::variants(&parsed_model),
    };
    let merged = merge_deep(
        Value::Object(
            variants
                .iter()
                .map(|(k, v)| (k.clone(), Value::Object(v.clone())))
                .collect(),
        ),
        Value::Object(
            model
                .variants
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| (k.clone(), Value::Object(v.as_value())))
                .collect(),
        ),
    );
    let merged = merged
        .as_object()
        .map(|map| {
            map.iter()
                .filter(|(_, v)| {
                    v.as_object()
                        .map(|o| o.get("disabled") != Some(&Value::Bool(true)))
                        .unwrap_or(true)
                })
                .map(|(k, v)| {
                    let mut value = v.clone();
                    if let Value::Object(o) = &mut value {
                        o.remove("disabled");
                    }
                    (k.clone(), value)
                })
                .collect::<Map<String, Value>>()
        })
        .unwrap_or_default();
    parsed_model.variants = merged
        .into_iter()
        .map(|(k, v)| (k, v.as_object().cloned().unwrap_or_default()))
        .collect();

    parsed.models.insert(model_id.to_string(), parsed_model);
}

/// Builds the provider registry without executable plugin model-hook results.
pub fn build_registry(input: &RegistryInput) -> Result<IndexMap<String, Info>, anyhow::Error> {
    build_registry_with_model_hooks(input, &[])
}

/// Builds the provider registry with materialized plugin model-hook results.
///
/// Hook model maps replace the catalog model map for an existing provider,
/// matching the reference `Hooks.provider.models` ordering. Config-defined
/// model entries are then merged over that result below, preserving the
/// declarative provider behavior already exposed by the registry.
pub fn build_registry_with_model_hooks(
    input: &RegistryInput,
    model_hooks: &[ProviderModelHookRegistration],
) -> Result<IndexMap<String, Info>, anyhow::Error> {
    build_registry_inner(input, model_hooks, None)
}

/// [`build_registry_with_model_hooks`] with an injectable npm-package
/// metadata resolver for config-declared providers.
///
/// The resolver supplies a default `api` base URL for models that name an npm
/// SDK package but supply no explicit URL and are absent from the models.dev
/// catalog. See [`NpmMetadata`].
pub fn build_registry_with_npm_metadata(
    input: &RegistryInput,
    model_hooks: &[ProviderModelHookRegistration],
    npm_metadata: Option<&dyn NpmMetadata>,
) -> Result<IndexMap<String, Info>, anyhow::Error> {
    build_registry_inner(input, model_hooks, npm_metadata)
}

fn build_registry_inner(
    input: &RegistryInput,
    model_hooks: &[ProviderModelHookRegistration],
    npm_metadata: Option<&dyn NpmMetadata>,
) -> Result<IndexMap<String, Info>, anyhow::Error> {
    let mut database: IndexMap<String, Info> = input
        .catalog
        .iter()
        .map(|(id, provider)| (id.clone(), from_models_dev_provider(provider)))
        .collect();

    let disabled: HashSet<&str> = input
        .config
        .disabled_providers
        .map(|list| list.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let enabled: Option<HashSet<&str>> = input
        .config
        .enabled_providers
        .map(|list| list.iter().map(String::as_str).collect());

    let is_provider_allowed = |provider_id: &str| -> bool {
        if let Some(enabled) = &enabled {
            if !enabled.contains(provider_id) {
                return false;
            }
        }
        !disabled.contains(provider_id)
    };

    // The reference executes provider model hooks after plugins are loaded
    // and before config models are applied. A typed registration is already a
    // host-owned result, so no callback is invoked in this pure registry.
    for hook in model_hooks {
        if disabled.contains(hook.provider_id.as_str()) {
            continue;
        }
        let Some(provider) = database.get_mut(&hook.provider_id) else {
            // Reference provider hooks only replace models for providers that
            // already exist in the catalog.
            continue;
        };
        provider.models = hook
            .models
            .iter()
            .map(|(model_id, model)| {
                let mut model = model.clone();
                model.id = model_id.clone();
                model.provider_id = hook.provider_id.clone();
                (model_id.clone(), model)
            })
            .collect();
    }

    // extend database from config
    for (provider_id, provider) in input.config.provider.iter() {
        let existing = database.get(provider_id).cloned();
        let models_dev = input.catalog.get(provider_id);
        let mut parsed = Info {
            id: provider_id.clone(),
            name: provider
                .name
                .clone()
                .or_else(|| existing.as_ref().map(|e| e.name.clone()))
                .unwrap_or_else(|| provider_id.clone()),
            env: provider
                .env
                .clone()
                .or_else(|| existing.as_ref().map(|e| e.env.clone()))
                .unwrap_or_default(),
            options: merge_deep(
                Value::Object(
                    existing
                        .as_ref()
                        .map(|e| e.options.clone())
                        .unwrap_or_default(),
                ),
                Value::Object(provider.options.clone().unwrap_or_default()),
            )
            .as_object()
            .unwrap()
            .clone(),
            source: Source::Config,
            key: None,
            models: existing
                .as_ref()
                .map(|e| e.models.clone())
                .unwrap_or_default(),
        };

        if let Some(models) = &provider.models {
            for (model_id, model) in models {
                merge_config_model(
                    &mut parsed,
                    model_id,
                    model,
                    Some(provider),
                    models_dev,
                    npm_metadata,
                );
            }
        }
        database.insert(provider_id.clone(), parsed);
    }

    let mut providers: IndexMap<String, Info> = IndexMap::new();

    // load env
    for (id, provider) in database.iter() {
        if disabled.contains(id.as_str()) {
            continue;
        }
        let api_key = provider
            .env
            .iter()
            .find_map(|item| env_get(input.envs, item));
        if api_key.is_none() {
            continue;
        }
        let mut patch = Map::new();
        patch.insert("source".to_string(), Value::from("env"));
        if provider.env.len() == 1 {
            if let Some(api_key) = api_key {
                patch.insert("key".to_string(), Value::from(api_key));
            }
        }
        merge_provider(&mut providers, &database, id, &patch);
    }

    // load api keys
    for (id, provider) in input.auths.iter() {
        if disabled.contains(id.as_str()) {
            continue;
        }
        if let AuthInfo::Api(api) = provider {
            let mut patch = Map::new();
            patch.insert("source".to_string(), Value::from("api"));
            patch.insert("key".to_string(), Value::from(api.key.clone()));
            merge_provider(&mut providers, &database, id, &patch);
        }
    }

    // custom loaders (may mutate the database entry, e.g. opencode pruning)
    let mut loader_results: Vec<(String, LoaderResult)> = Vec::new();
    for (id, provider) in database.iter_mut() {
        if disabled.contains(id.as_str()) {
            continue;
        }
        if let Some(result) = custom_loader(id, provider, input)? {
            if result.autoload || providers.contains_key(id) {
                loader_results.push((id.clone(), result));
            }
        }
    }
    for (id, result) in loader_results {
        let mut patch = Map::new();
        patch.insert("options".to_string(), Value::Object(result.options));
        if !providers.contains_key(&id) {
            patch.insert("source".to_string(), Value::from("custom"));
        }
        merge_provider(&mut providers, &database, &id, &patch);
    }

    // re-apply config over the merged registry
    for (id, provider) in input.config.provider.iter() {
        let mut patch = Map::new();
        patch.insert("source".to_string(), Value::from("config"));
        if let Some(env) = &provider.env {
            patch.insert("env".to_string(), Value::from(env.clone()));
        }
        if let Some(name) = &provider.name {
            patch.insert("name".to_string(), Value::from(name.clone()));
        }
        if let Some(options) = &provider.options {
            patch.insert("options".to_string(), Value::Object(options.clone()));
        }
        merge_provider(&mut providers, &database, id, &patch);
    }

    // final filtering
    let mut result = IndexMap::new();
    for (id, mut provider) in providers {
        if !is_provider_allowed(&id) {
            continue;
        }
        let config_provider = input.config.provider.get(&id);

        let mut removed = Vec::new();
        for (model_id, model) in provider.models.iter_mut() {
            let is_openai_chat_alias = (model_id == "gpt-5-chat-latest"
                && ["openai", "github-copilot", "openrouter"].contains(&id.as_str()))
                || (id == "openrouter" && model_id == "openai/gpt-5-chat");
            if is_openai_chat_alias {
                removed.push(model_id.clone());
                continue;
            }
            if model.status == ModelStatus::Alpha && !input.enable_experimental_models {
                removed.push(model_id.clone());
                continue;
            }
            if model.status == ModelStatus::Deprecated {
                removed.push(model_id.clone());
                continue;
            }
            if let Some(config_provider) = config_provider {
                if let Some(blacklist) = &config_provider.blacklist {
                    if blacklist.contains(model_id) {
                        removed.push(model_id.clone());
                        continue;
                    }
                }
                if let Some(whitelist) = &config_provider.whitelist {
                    if !whitelist.contains(model_id) {
                        removed.push(model_id.clone());
                        continue;
                    }
                }
            }

            if model.variants.is_empty() {
                model.variants = transform::variants(model);
            }
            if let Some(config_provider) = config_provider {
                if let Some(config_variants) = config_provider
                    .models
                    .as_ref()
                    .and_then(|models| models.get(model_id))
                    .and_then(|m| m.variants.as_ref())
                {
                    let merged = merge_deep(
                        Value::Object(
                            model
                                .variants
                                .iter()
                                .map(|(k, v)| (k.clone(), Value::Object(v.clone())))
                                .collect(),
                        ),
                        Value::Object(
                            config_variants
                                .iter()
                                .map(|(k, v)| (k.clone(), Value::Object(v.as_value())))
                                .collect(),
                        ),
                    );
                    let filtered = merged
                        .as_object()
                        .map(|map| {
                            map.iter()
                                .filter(|(_, v)| {
                                    v.as_object()
                                        .map(|o| o.get("disabled") != Some(&Value::Bool(true)))
                                        .unwrap_or(true)
                                })
                                .map(|(k, v)| {
                                    let mut value = v.clone();
                                    if let Value::Object(o) = &mut value {
                                        o.remove("disabled");
                                    }
                                    (k.clone(), value)
                                })
                                .collect::<Map<String, Value>>()
                        })
                        .unwrap_or_default();
                    model.variants = filtered
                        .into_iter()
                        .map(|(k, v)| (k, v.as_object().cloned().unwrap_or_default()))
                        .collect();
                }
            }
        }
        for model_id in removed {
            provider.models.shift_remove(&model_id);
        }

        if provider.models.is_empty() {
            continue;
        }
        result.insert(id, provider);
    }

    Ok(result)
}
