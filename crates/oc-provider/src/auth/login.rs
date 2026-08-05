//! `opencode auth login` flow logic.
//!
//! From reference/packages/opencode/src/cli/cmd/providers.ts. The CLI prompts
//! and process spawning are abstracted behind traits so oc-cli can bind the
//! real TUI; the selection, authorization, and credential-storage logic is
//! shared and testable here.

use std::collections::BTreeMap;

use super::{AuthStore, Info};
use crate::provider::auth::{
    AuthCallbackResult, AuthHook, CallbackMethod, MethodType, Prompt as AuthPrompt,
};

/// `resolvePluginProviders` from `providers.ts`.
pub fn resolve_plugin_providers<H>(
    hooks: &[H],
    existing_providers: &BTreeMap<String, Info>,
    disabled: &std::collections::HashSet<String>,
    enabled: &Option<std::collections::HashSet<String>>,
    provider_names: &BTreeMap<String, String>,
) -> Vec<(String, String)>
where
    H: HasAuth,
{
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for hook in hooks {
        let Some(id) = hook.auth_provider() else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        if existing_providers.contains_key(id) {
            continue;
        }
        if disabled.contains(id) {
            continue;
        }
        if let Some(enabled) = enabled {
            if !enabled.contains(id) {
                continue;
            }
        }
        let name = provider_names
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string());
        result.push((id.to_string(), name));
    }
    result
}

/// The subset of a plugin's `auth` hook that the login flow reads.
pub trait HasAuth {
    fn auth_provider(&self) -> Option<&str>;
}

/// A validate callback for a prompt, mirroring the reference `validate` option.
type Validator<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

/// Interactive prompt/UI surface used by the login flows.
///
/// TODO(integration): bind to the real TUI in oc-cli (Prompts from
/// `cli/effect/prompt.ts` and `cli/ui.ts`).
pub trait LoginPrompt: Send + Sync {
    fn intro(&self, title: &str);
    fn log_info(&self, message: &str);
    fn log_warn(&self, message: &str);
    fn log_error(&self, message: &str);
    fn log_success(&self, message: &str);
    fn outro(&self, message: &str);
    fn text(
        &self,
        message: &str,
        placeholder: Option<&str>,
        validate: Validator<'_>,
    ) -> Option<String>;
    fn password(&self, message: &str) -> Option<String>;
    fn select(&self, message: &str, options: &[(String, String)]) -> Option<usize>;
    fn autocomplete(&self, message: &str, options: &[(String, String)]) -> Option<String>;
    fn spinner_start(&self, message: &str);
    fn spinner_stop(&self, message: &str, failed: bool);
}

fn prompt_value<T>(value: Option<T>) -> Result<T, LoginError> {
    value.ok_or(LoginError::Cancelled)
}

/// Errors raised by the login flows.
#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
    #[error("{0}: {1}")]
    FailedWithCause(String, String),
}

impl LoginError {
    fn from_cli(message: &str, cause: anyhow::Error) -> LoginError {
        LoginError::FailedWithCause(message.to_string(), cause.to_string())
    }
}

/// Runs the `login [url]` well-known flow.
///
/// From the `args.url` branch of `Cli.providers.login` in `providers.ts`.
/// `fetch_wellknown` fetches `{url}/.well-known/opencode` and `run_command`
/// executes the auth command, capturing stdout.
pub fn login_url(
    url: &str,
    auth: &mut impl AuthStore,
    prompt: &dyn LoginPrompt,
    fetch_wellknown: impl Fn(&str) -> Result<WellKnownMetadata, anyhow::Error>,
    run_command: impl Fn(&[String]) -> Result<(i32, String), anyhow::Error>,
) -> Result<(), LoginError> {
    let url = url.trim_end_matches('/');
    let wellknown = fetch_wellknown(url).map_err(|e| {
        LoginError::from_cli(
            &format!("Failed to load auth provider metadata from {}: ", url),
            e,
        )
    })?;
    prompt.log_info(&format!("Running `{}`", wellknown.auth.command.join(" ")));

    let (exit, token) = run_command(&wellknown.auth.command)
        .map_err(|e| LoginError::from_cli("Failed to run auth provider command: ", e))?;
    if exit != 0 {
        prompt.log_error("Failed");
        prompt.outro("Done");
        return Ok(());
    }
    auth.set(
        url,
        Info::WellKnown(super::WellKnown {
            key: wellknown.auth.env.clone(),
            token: token.trim().to_string(),
        }),
    )
    .map_err(|e| LoginError::Failed(e.to_string()))?;
    prompt.log_success(&format!("Logged into {}", url));
    prompt.outro("Done");
    Ok(())
}

/// `WellKnownMetadata` from the `/.well-known/opencode` response.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct WellKnownMetadata {
    pub auth: WellKnownAuth,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct WellKnownAuth {
    pub command: Vec<String>,
    pub env: String,
}

/// Options for the interactive `login` flow.
pub struct LoginOptions {
    pub provider: Option<String>,
    pub method: Option<String>,
}

/// Runs the interactive `login` flow for a catalog-backed provider or a plugin
/// auth provider.
///
/// From `Cli.providers.login` in `providers.ts`. `catalog_providers` maps
/// provider ID to (name, env). `hooks` provides plugin auth hooks keyed by
/// provider ID.
pub fn login(
    auth: &mut impl AuthStore,
    prompt: &dyn LoginPrompt,
    options: &LoginOptions,
    catalog_providers: &BTreeMap<String, CatalogProvider>,
    plugin_hooks: &BTreeMap<String, Box<dyn AuthHook>>,
    disabled: &std::collections::HashSet<String>,
    enabled: &Option<std::collections::HashSet<String>>,
) -> Result<(), LoginError> {
    prompt.intro("Add credential");

    let priority: BTreeMap<&str, usize> = BTreeMap::from([
        ("opencode", 0),
        ("openai", 1),
        ("github-copilot", 2),
        ("google", 3),
        ("anthropic", 4),
        ("openrouter", 5),
        ("vercel", 6),
    ]);

    let mut providers: Vec<(String, CatalogProvider)> = catalog_providers
        .iter()
        .filter(|(id, _)| {
            let allowed = match enabled {
                Some(enabled) => enabled.contains(*id),
                None => true,
            };
            allowed && !disabled.contains(*id)
        })
        .map(|(id, provider)| (id.clone(), provider.clone()))
        .collect();
    providers.sort_by(|a, b| {
        let a_priority = priority.get(a.0.as_str()).copied().unwrap_or(99);
        let b_priority = priority.get(b.0.as_str()).copied().unwrap_or(99);
        a_priority
            .cmp(&b_priority)
            .then_with(|| a.1.name.cmp(&b.1.name))
    });

    let existing: BTreeMap<String, Info> =
        auth.all().map_err(|e| LoginError::Failed(e.to_string()))?;
    let _ = &existing;

    let mut options_list: Vec<(String, String)> = providers
        .iter()
        .map(|(id, provider)| (id.clone(), provider.name.clone()))
        .collect();
    options_list.push(("other".to_string(), "Other".to_string()));

    let provider = match &options.provider {
        Some(input) => {
            let by_id = providers.iter().find(|(id, _)| id == input);
            let by_name = providers
                .iter()
                .find(|(_, p)| p.name.to_lowercase() == input.to_lowercase());
            let matched = by_id.or(by_name).map(|(id, _)| id.clone());
            match matched {
                Some(id) => id,
                None => {
                    // Plugin auth providers can also be addressed by name.
                    if let Some((id, _)) = plugin_hooks
                        .iter()
                        .find(|(id, _)| *id == input || id.to_lowercase() == input.to_lowercase())
                    {
                        id.clone()
                    } else {
                        return Err(LoginError::Failed(format!(
                            "Unknown provider \"{}\"",
                            input
                        )));
                    }
                }
            }
        }
        None => prompt_value(prompt.autocomplete("Select provider", &options_list))?,
    };

    if let Some(hook) = plugin_hooks.get(&provider) {
        let handled = handle_plugin_auth(
            auth,
            prompt,
            hook.as_ref(),
            &provider,
            options.method.as_deref(),
        )?;
        if handled {
            return Ok(());
        }
    }

    if provider == "other" {
        let entered = prompt_value(prompt.text(
            "Enter provider id",
            None,
            Some(&|value: &str| {
                if value.is_empty()
                    || !value
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    Some("a-z, 0-9 and hyphens only".to_string())
                } else {
                    None
                }
            }),
        ))?;
        let custom = entered.trim_start_matches("@ai-sdk/").to_string();
        if let Some(hook) = plugin_hooks.get(&custom) {
            let handled = handle_plugin_auth(
                auth,
                prompt,
                hook.as_ref(),
                &custom,
                options.method.as_deref(),
            )?;
            if handled {
                return Ok(());
            }
        }
        prompt.log_warn(&format!(
            "This only stores a credential for {} - you will need configure it in opencode.json, check the docs for examples.",
            custom
        ));
        let key = prompt_value(prompt.password("Enter your API key"))?;
        auth.set(
            &custom,
            Info::Api(super::Api {
                key,
                metadata: None,
            }),
        )
        .map_err(|e| LoginError::Failed(e.to_string()))?;
        prompt.outro("Done");
        return Ok(());
    }

    if provider == "amazon-bedrock" {
        prompt.log_info(
            "Amazon Bedrock authentication priority:\n  \
             1. Bearer token (AWS_BEARER_TOKEN_BEDROCK or /connect)\n  \
             2. AWS credential chain (profile, access keys, IAM roles, EKS IRSA)\n\n\
             Configure via opencode.json options (profile, region, endpoint) or\n\
             AWS environment variables (AWS_PROFILE, AWS_REGION, AWS_ACCESS_KEY_ID, AWS_WEB_IDENTITY_TOKEN_FILE).",
        );
    }
    if provider == "opencode" {
        prompt.log_info("Create an api key at https://opencode.ai/auth");
    }
    if provider == "vercel" {
        prompt.log_info("You can create an api key at https://vercel.link/ai-gateway-token");
    }
    if ["cloudflare", "cloudflare-ai-gateway"].contains(&provider.as_str()) {
        prompt.log_info(
            "Cloudflare AI Gateway can be configured with CLOUDFLARE_GATEWAY_ID, CLOUDFLARE_ACCOUNT_ID, and CLOUDFLARE_API_TOKEN environment variables. Read more: https://opencode.ai/docs/providers/#cloudflare-ai-gateway",
        );
    }

    let key = prompt_value(prompt.password("Enter your API key"))?;
    auth.set(
        &provider,
        Info::Api(super::Api {
            key,
            metadata: None,
        }),
    )
    .map_err(|e| LoginError::Failed(e.to_string()))?;
    prompt.outro("Done");
    Ok(())
}

/// Drives a plugin auth hook through prompt collection, OAuth or API-key
/// authorization, and credential storage.
///
/// From `handlePluginAuth` in `providers.ts`.
pub fn handle_plugin_auth(
    auth: &mut impl AuthStore,
    prompt: &dyn LoginPrompt,
    hook: &dyn AuthHook,
    provider: &str,
    method_name: Option<&str>,
) -> Result<bool, LoginError> {
    let methods = hook.methods();
    let index = match method_name {
        None => {
            if methods.len() <= 1 {
                0
            } else {
                prompt_value(
                    prompt.select(
                        "Login method",
                        &methods
                            .iter()
                            .enumerate()
                            .map(|(i, m)| (i.to_string(), m.label.clone()))
                            .collect::<Vec<_>>(),
                    ),
                )?
            }
        }
        Some(name) => {
            let match_index = methods
                .iter()
                .position(|m| m.label.to_lowercase() == name.to_lowercase());
            match match_index {
                Some(index) => index,
                None => {
                    let available = methods
                        .iter()
                        .map(|m| m.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(LoginError::Failed(format!(
                        "Unknown method \"{}\" for {}. Available: {}",
                        name, provider, available
                    )));
                }
            }
        }
    };
    let method = &methods[index];

    let mut inputs: BTreeMap<String, String> = BTreeMap::new();
    if let Some(prompts) = &method.prompts {
        for auth_prompt in prompts {
            let when = match auth_prompt {
                AuthPrompt::Text(p) => &p.when,
                AuthPrompt::Select(p) => &p.when,
            };
            if let Some(when) = when {
                let Some(value) = inputs.get(&when.key) else {
                    continue;
                };
                let matches = match when.op {
                    crate::provider::auth::WhenOp::Eq => *value == when.value,
                    crate::provider::auth::WhenOp::Neq => *value != when.value,
                };
                if !matches {
                    continue;
                }
            }
            let key = auth_prompt.key();
            let value = match auth_prompt {
                AuthPrompt::Select(p) => {
                    let options: Vec<(String, String)> = p
                        .options
                        .iter()
                        .map(|o| (o.value.clone(), o.label.clone()))
                        .collect();
                    let selected = prompt_value(prompt.select(&p.message, &options))?;
                    options[selected].0.clone()
                }
                AuthPrompt::Text(p) => {
                    prompt_value(prompt.text(&p.message, p.placeholder.as_deref(), None))?
                }
            };
            inputs.insert(key.to_string(), value);
        }
    }

    match method.r#type {
        MethodType::OAuth => {
            let authorize = hook
                .authorize(index, &inputs)
                .map_err(|e| LoginError::from_cli("Failed to authorize: ", e))?;
            if !authorize.url.is_empty() {
                prompt.log_info(&format!("Go to: {}", authorize.url));
            }

            match authorize.method {
                CallbackMethod::Auto => {
                    if !authorize.instructions.is_empty() {
                        prompt.log_info(&authorize.instructions);
                    }
                    prompt.spinner_start("Waiting for authorization...");
                    let result = hook
                        .callback(None)
                        .map_err(|e| LoginError::from_cli("Failed to authorize: ", e))?;
                    match result {
                        AuthCallbackResult::Failed => {
                            prompt.spinner_stop("Failed to authorize", true);
                        }
                        AuthCallbackResult::Success { oauth, api, .. } => {
                            store_oauth_or_api(auth, provider, oauth, api)?;
                            prompt.spinner_stop("Login successful", false);
                        }
                    }
                }
                CallbackMethod::Code => {
                    let code = prompt_value(prompt.text(
                        "Paste the authorization code here: ",
                        None,
                        Some(&|value: &str| {
                            if value.is_empty() {
                                Some("Required".to_string())
                            } else {
                                None
                            }
                        }),
                    ))?;
                    let result = hook
                        .callback(Some(&code))
                        .map_err(|e| LoginError::from_cli("Failed to authorize: ", e))?;
                    match result {
                        AuthCallbackResult::Failed => prompt.log_error("Failed to authorize"),
                        AuthCallbackResult::Success { oauth, api, .. } => {
                            store_oauth_or_api(auth, provider, oauth, api)?;
                            prompt.log_success("Login successful");
                        }
                    }
                }
            }
            prompt.outro("Done");
            Ok(true)
        }
        MethodType::Api => {
            let key = prompt_value(prompt.password("Enter your API key"))?;
            let metadata = if inputs.is_empty() {
                None
            } else {
                Some(inputs.clone())
            };
            let result = hook
                .callback(Some(&key))
                .map_err(|e| LoginError::from_cli("Failed to authorize: ", e))?;
            match result {
                AuthCallbackResult::Failed => prompt.log_error("Failed to authorize"),
                AuthCallbackResult::Success { oauth: _, api, .. } => {
                    let api = api.unwrap_or(crate::provider::auth::ApiCredential {
                        key,
                        metadata: None,
                    });
                    let merged_metadata = match (metadata, api.metadata) {
                        (Some(mut a), Some(b)) => {
                            a.extend(b);
                            Some(a)
                        }
                        (Some(a), None) => Some(a),
                        (None, b) => b,
                    };
                    auth.set(
                        provider,
                        Info::Api(super::Api {
                            key: api.key,
                            metadata: merged_metadata,
                        }),
                    )
                    .map_err(|e| LoginError::Failed(e.to_string()))?;
                    prompt.log_success("Login successful");
                }
            }
            prompt.outro("Done");
            Ok(true)
        }
    }
}

fn store_oauth_or_api(
    auth: &mut impl AuthStore,
    provider: &str,
    oauth: Option<crate::provider::auth::OAuthCredential>,
    api: Option<crate::provider::auth::ApiCredential>,
) -> Result<(), LoginError> {
    if let Some(oauth) = oauth {
        auth.set(
            provider,
            Info::Oauth(super::Oauth {
                refresh: oauth.refresh,
                access: oauth.access,
                expires: oauth.expires,
                account_id: oauth.account_id,
                enterprise_url: oauth.enterprise_url,
            }),
        )
        .map_err(|e| LoginError::Failed(e.to_string()))?;
    }
    if let Some(api) = api {
        auth.set(
            provider,
            Info::Api(super::Api {
                key: api.key,
                metadata: api.metadata,
            }),
        )
        .map_err(|e| LoginError::Failed(e.to_string()))?;
    }
    Ok(())
}

/// A catalog provider used by the login flow.
#[derive(Debug, Clone)]
pub struct CatalogProvider {
    pub name: String,
    pub env: Vec<String>,
}

impl CatalogProvider {
    pub fn from_registry(info: &crate::provider::Info) -> CatalogProvider {
        CatalogProvider {
            name: info.name.clone(),
            env: info.env.clone(),
        }
    }
}

/// The `logout` flow.
///
/// From `Cli.providers.logout` in `providers.ts`.
pub fn logout(
    auth: &mut impl AuthStore,
    prompt: &dyn LoginPrompt,
    requested: Option<&str>,
    names: &BTreeMap<String, String>,
) -> Result<(), LoginError> {
    prompt.intro("Remove credential");
    let credentials = auth.all().map_err(|e| LoginError::Failed(e.to_string()))?;
    if credentials.is_empty() {
        prompt.log_error("No credentials found");
        return Ok(());
    }
    let options: Vec<(String, String)> = credentials
        .iter()
        .map(|(key, value)| {
            let name = names.get(key).cloned().unwrap_or_else(|| key.clone());
            (key.clone(), format!("{} ({})", name, value.r#type()))
        })
        .collect();

    let provider = match requested {
        Some(requested) => options
            .iter()
            .find(|(key, _)| {
                key == requested
                    || names.get(key).map(|n| n.to_lowercase()) == Some(requested.to_lowercase())
            })
            .map(|(key, _)| key.clone())
            .ok_or_else(|| {
                LoginError::Failed(format!("Unknown configured provider \"{}\"", requested))
            })?,
        None => prompt_value(prompt.autocomplete("Select provider", &options))?,
    };
    auth.remove(&provider)
        .map_err(|e| LoginError::Failed(e.to_string()))?;
    prompt.outro("Logout successful");
    Ok(())
}

/// Provider discovery for `login`, mirroring the models.dev catalog filtering
/// in `providers.ts`.
pub fn catalog_providers(
    catalog: &BTreeMap<String, crate::models_dev::Provider>,
    enabled: &Option<std::collections::HashSet<String>>,
    disabled: &std::collections::HashSet<String>,
) -> BTreeMap<String, CatalogProvider> {
    catalog
        .iter()
        .filter(|(id, _)| {
            let allowed = match enabled {
                Some(enabled) => enabled.contains(*id),
                None => true,
            };
            allowed && !disabled.contains(*id)
        })
        .map(|(id, provider)| {
            (
                id.clone(),
                CatalogProvider {
                    name: provider.name.clone(),
                    env: provider.env.clone(),
                },
            )
        })
        .collect()
}

/// Registers the given provider as connected (used by `list`/`providers`).
pub fn connected_providers(
    catalog: &BTreeMap<String, crate::models_dev::Provider>,
    envs: &BTreeMap<String, Option<String>>,
) -> Vec<String> {
    catalog
        .iter()
        .filter(|(_, provider)| {
            provider
                .env
                .iter()
                .any(|key| envs.get(key).is_some_and(|v| v.is_some()))
        })
        .map(|(id, _)| id.clone())
        .collect()
}
