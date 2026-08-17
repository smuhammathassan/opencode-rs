//! Provider handler. From reference/packages/server/src/handlers/provider.ts.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{request_location, HandlerResult};
use crate::errors::ApiError;
use indexmap::IndexMap;
use std::collections::HashMap;

/// Read the same global provider credential file used by the CLI.
///
/// Keeping this at the server boundary means the registry, `/provider`, and
/// the session runner all see credentials written by `opencode auth login`.
pub(crate) fn provider_auths() -> std::collections::BTreeMap<String, oc_provider::auth::Info> {
    use oc_provider::auth::AuthStore;

    oc_provider::auth::FileAuthStore::new(oc_mcp::auth::default_data_dir())
        .all()
        .unwrap_or_default()
}

/// Resolve a persisted provider credential into the transport auth policy
/// consumed by `oc-llm`. The reference keeps this lookup at the provider
/// boundary, so credentials written by the CLI are usable by server sessions
/// without copying secrets into process environment variables.
pub(crate) fn provider_auth(provider_id: &str) -> Option<oc_llm::route::auth::Auth> {
    let info = provider_auths().remove(provider_id)?;
    Some(provider_auth_from_info(provider_id, info))
}

pub(crate) fn provider_uses_oauth(provider_id: &str) -> bool {
    matches!(
        provider_auths().get(provider_id),
        Some(oc_provider::auth::Info::Oauth(_))
    )
}

/// Resolve a provider-specific OAuth transport endpoint from persisted
/// credential metadata. GitHub Enterprise Copilot uses a different API host;
/// public GitHub Copilot keeps the provider's normal default endpoint.
pub(crate) fn provider_oauth_base_url(provider_id: &str) -> Option<String> {
    let auths = provider_auths();
    let Some(oc_provider::auth::Info::Oauth(oauth)) = auths.get(provider_id) else {
        return None;
    };
    (provider_id == "github-copilot")
        .then(|| oauth.enterprise_url.as_deref())
        .flatten()
        .filter(|domain| !domain.trim().is_empty())
        .map(|domain| format!("https://copilot-api.{}", domain.trim_end_matches('/')))
}

/// Refresh an expired persisted OAuth credential before a live provider call.
/// API-key, well-known, missing, valid, and unsupported-refresh credentials are
/// deliberately no-ops; only a host-owned refresh hook can rotate a token.
pub(crate) fn refresh_provider_auth(
    state: &crate::state::AppState,
    provider_id: &str,
) -> Result<oc_provider::provider::auth::OauthRefreshResult, String> {
    use oc_provider::auth::AuthStore;

    let mut store = oc_provider::auth::FileAuthStore::new(oc_mcp::auth::default_data_dir());
    let Some(info) = store.get(provider_id).map_err(|error| error.to_string())? else {
        return Ok(oc_provider::provider::auth::OauthRefreshResult::NotNeeded);
    };
    if !matches!(info, oc_provider::auth::Info::Oauth(_)) {
        return Ok(oc_provider::provider::auth::OauthRefreshResult::NotNeeded);
    }
    let now = crate::state::now_millis().max(0) as u64;
    state
        .provider_auth
        .refresh(provider_id, now, &mut store)
        .map_err(|error| error.to_string())
}

fn provider_auth_from_info(
    provider_id: &str,
    info: oc_provider::auth::Info,
) -> oc_llm::route::auth::Auth {
    let (value, oauth) = match info {
        oc_provider::auth::Info::Oauth(oauth) => {
            let bearer = oc_llm::route::auth::Auth::value(oauth.access).bearer_auth();
            if provider_id != "openai" && provider_id != "github-copilot" {
                return bearer;
            }
            let mut headers = oc_llm::route::auth::HeaderMap::new();
            if provider_id == "openai" {
                headers.insert("originator".to_string(), "opencode".to_string());
                headers.insert("User-Agent".to_string(), "opencode-rust".to_string());
                if let Some(account_id) = oauth.account_id {
                    headers.insert("ChatGPT-Account-Id".to_string(), account_id);
                }
            } else {
                headers.insert("X-GitHub-Api-Version".to_string(), "2026-06-01".to_string());
                headers.insert(
                    "Openai-Intent".to_string(),
                    "conversation-edits".to_string(),
                );
                headers.insert("User-Agent".to_string(), "opencode-rust".to_string());
            }
            return bearer.and_then(oc_llm::route::auth::Auth::headers(headers));
        }
        oc_provider::auth::Info::Api(api) => (api.key, false),
        oc_provider::auth::Info::WellKnown(well_known) => (well_known.token, false),
    };
    let credential = oc_llm::route::auth::Auth::value(value);
    if oauth {
        return credential.bearer_auth();
    }
    match provider_id {
        "anthropic" => credential.header_auth("x-api-key"),
        "google" => credential.header_auth("x-goog-api-key"),
        _ => credential.bearer_auth(),
    }
}

/// A secret-free projection for the v1 provider response.
pub(crate) fn authenticated_provider_ids() -> serde_json::Map<String, serde_json::Value> {
    provider_auths()
        .into_iter()
        .map(|(provider, info)| {
            (
                provider,
                serde_json::Value::String(info.r#type().to_string()),
            )
        })
        .collect()
}

/// Build the connected provider registry from the embedded models.dev catalog
/// and the process environment. Config/auth stores can be layered in here as
/// they become server-scoped services.
#[allow(dead_code)]
pub(crate) fn provider_values_from_config(config: &serde_json::Value) -> Vec<serde_json::Value> {
    provider_catalog_from_config(config)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|provider| serde_json::to_value(provider).ok())
        .collect()
}

pub(crate) fn provider_values_from_state_config(
    state: &crate::state::AppState,
    config: &serde_json::Value,
) -> Vec<serde_json::Value> {
    provider_catalog_from_config_with_model_hooks(
        config,
        &crate::plugin_registry::plugin_model_hooks(state),
    )
    .unwrap_or_default()
    .into_iter()
    .filter_map(|provider| serde_json::to_value(provider).ok())
    .collect()
}

/// Build the typed, public provider catalog used by the v2 provider/model
/// endpoints. The registry owns models.dev conversion and config merging; the
/// server only supplies the current config, environment, and persisted auth.
#[allow(dead_code)]
pub(crate) fn provider_catalog_from_config(
    config: &serde_json::Value,
) -> Result<Vec<oc_provider::provider::Info>, ApiError> {
    provider_catalog_from_config_with_model_hooks(config, &[])
}

pub(crate) fn provider_catalog_from_config_with_model_hooks(
    config: &serde_json::Value,
    model_hooks: &[oc_provider::provider::registry::ProviderModelHookRegistration],
) -> Result<Vec<oc_provider::provider::Info>, ApiError> {
    let Ok(catalog) = oc_provider::models_dev::snapshot() else {
        return Err(ApiError::Unknown {
            message: "failed to load embedded provider catalog".into(),
            reference: None,
        });
    };
    let envs = std::env::vars()
        .map(|(key, value)| (key, Some(value)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let auths = provider_auths();
    let providers = config
        .get("provider")
        .and_then(serde_json::Value::as_object)
        .map(|values| {
            values
                .iter()
                .filter_map(|(id, value)| {
                    serde_json::from_value::<oc_provider::provider::registry::ConfigProvider>(
                        value.clone(),
                    )
                    .ok()
                    .map(|mut provider| {
                        if provider.id.is_none() {
                            provider.id = Some(id.clone());
                        }
                        (id.clone(), provider)
                    })
                })
                .collect::<IndexMap<_, _>>()
        })
        .unwrap_or_default();
    let disabled = config.get("disabled_providers").and_then(string_list);
    let enabled = config.get("enabled_providers").and_then(string_list);
    let input = oc_provider::provider::registry::RegistryInput {
        catalog: &catalog,
        config: oc_provider::provider::registry::ConfigInput {
            provider: &providers,
            disabled_providers: disabled.as_deref(),
            enabled_providers: enabled.as_deref(),
        },
        envs: &envs,
        auths: &auths,
        enable_experimental_models: false,
    };
    let providers =
        oc_provider::provider::registry::build_registry_with_model_hooks(&input, model_hooks)
            .map_err(|error| ApiError::Unknown {
                message: format!("failed to build provider catalog: {error}"),
                reference: None,
            })?;
    Ok(providers
        .values()
        .map(oc_provider::provider::to_public_info)
        .collect())
}

fn string_list(value: &serde_json::Value) -> Option<Vec<String>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{provider_auth_from_info, refresh_provider_auth};
    use crate::auth::AuthConfig;
    use crate::cors::CorsOptions;
    use crate::location::Location;
    use oc_llm::route::auth::{Auth, Credential, HeaderRender};
    use oc_provider::auth::{Api, AuthStore, Info, Oauth};
    use oc_provider::provider::auth::{
        AuthCallbackResult, AuthHook, AuthOAuthResult, Method, OAuthCredential,
    };
    use std::collections::BTreeMap;

    #[test]
    fn maps_api_keys_to_provider_specific_headers() {
        let Auth::Credential { credential, render } = provider_auth_from_info(
            "anthropic",
            Info::Api(Api {
                key: "secret".into(),
                metadata: None,
            }),
        ) else {
            panic!("expected credential auth")
        };
        assert!(matches!(credential, Credential::Value(value) if value == "secret"));
        assert!(matches!(render, HeaderRender::Header(name) if name == "x-api-key"));
    }

    #[test]
    fn maps_oauth_access_tokens_to_bearer_auth() {
        let Auth::Credential { credential, render } = provider_auth_from_info(
            "anthropic",
            Info::Oauth(Oauth {
                refresh: "refresh".into(),
                access: "access".into(),
                expires: 123,
                account_id: None,
                enterprise_url: None,
            }),
        ) else {
            panic!("expected credential auth")
        };
        assert!(matches!(credential, Credential::Value(value) if value == "access"));
        assert!(matches!(render, HeaderRender::Bearer));
    }

    #[test]
    fn maps_openai_oauth_account_headers_for_codex() {
        let auth = provider_auth_from_info(
            "openai",
            Info::Oauth(Oauth {
                refresh: "refresh".into(),
                access: "access".into(),
                expires: u64::MAX,
                account_id: Some("acct_test".into()),
                enterprise_url: None,
            }),
        );
        let Auth::AndThen(_, headers) = auth else {
            panic!("expected bearer plus Codex headers")
        };
        let Auth::Headers(headers) = *headers else {
            panic!("expected static Codex headers")
        };
        assert_eq!(
            headers.get("ChatGPT-Account-Id"),
            Some(&"acct_test".to_string())
        );
        assert_eq!(headers.get("originator"), Some(&"opencode".to_string()));
    }

    #[test]
    fn maps_github_copilot_oauth_headers() {
        let auth = provider_auth_from_info(
            "github-copilot",
            Info::Oauth(Oauth {
                refresh: "refresh".into(),
                access: "access".into(),
                expires: 0,
                account_id: None,
                enterprise_url: Some("company.ghe.com".into()),
            }),
        );
        let Auth::AndThen(_, headers) = auth else {
            panic!("expected bearer plus Copilot headers")
        };
        let Auth::Headers(headers) = *headers else {
            panic!("expected static Copilot headers")
        };
        assert_eq!(
            headers.get("X-GitHub-Api-Version"),
            Some(&"2026-06-01".to_string())
        );
        assert_eq!(
            headers.get("Openai-Intent"),
            Some(&"conversation-edits".to_string())
        );
    }

    struct RefreshHook;

    impl AuthHook for RefreshHook {
        fn methods(&self) -> Vec<Method> {
            Vec::new()
        }

        fn validate(&self, _method_index: usize, _key: &str, _value: &str) -> Option<String> {
            None
        }

        fn authorize(
            &self,
            _method_index: usize,
            _inputs: &BTreeMap<String, String>,
        ) -> Result<AuthOAuthResult, anyhow::Error> {
            Err(anyhow::anyhow!("not used in refresh test"))
        }

        fn callback(&self, _code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error> {
            Err(anyhow::anyhow!("not used in refresh test"))
        }

        fn refresh(&self, _credential: &Oauth) -> Result<Option<OAuthCredential>, anyhow::Error> {
            Ok(Some(OAuthCredential {
                refresh: "rotated-refresh".into(),
                access: "rotated-access".into(),
                expires: u64::MAX,
                account_id: None,
                enterprise_url: None,
            }))
        }
    }

    #[test]
    fn live_provider_refresh_rotates_persisted_credentials() {
        let previous_home = std::env::var_os("OPENCODE_TEST_HOME");
        let home =
            std::env::temp_dir().join(format!("opencode-provider-refresh-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("OPENCODE_TEST_HOME", &home);

        let mut store = oc_provider::auth::FileAuthStore::new(oc_mcp::auth::default_data_dir());
        store
            .set(
                "refresh-provider",
                Info::Oauth(Oauth {
                    refresh: "old-refresh".into(),
                    access: "old-access".into(),
                    expires: 1,
                    account_id: None,
                    enterprise_url: None,
                }),
            )
            .unwrap();
        let state = crate::state::AppState::new_with_provider_auth(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
            BTreeMap::from([(
                "refresh-provider".into(),
                Box::new(RefreshHook) as Box<dyn AuthHook>,
            )]),
        );

        assert_eq!(
            refresh_provider_auth(&state, "refresh-provider").unwrap(),
            oc_provider::provider::auth::OauthRefreshResult::Refreshed
        );
        let refreshed = store.get("refresh-provider").unwrap().unwrap();
        let Info::Oauth(refreshed) = refreshed else {
            panic!("expected OAuth credential")
        };
        assert_eq!(refreshed.access, "rotated-access");
        assert_eq!(refreshed.refresh, "rotated-refresh");

        if let Some(previous_home) = previous_home {
            std::env::set_var("OPENCODE_TEST_HOME", previous_home);
        } else {
            std::env::remove_var("OPENCODE_TEST_HOME");
        }
        let _ = std::fs::remove_dir_all(home);
    }
}

/// `catalog.provider.available()` from `reference/packages/server/src/handlers/provider.ts`.
pub async fn provider_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    let hooks = crate::plugin_registry::plugin_model_hooks(&state);
    let providers = provider_catalog_from_config_with_model_hooks(&config, &hooks)?;
    super::json(&crate::schema::LocationResponse {
        location: location.info(),
        data: providers,
    })
}

/// `catalog.provider.get(...)` from `reference/packages/server/src/handlers/provider.ts`.
pub async fn provider_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let _location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    let hooks = crate::plugin_registry::plugin_model_hooks(&state);
    let providers = provider_catalog_from_config_with_model_hooks(&config, &hooks)?;
    let provider_id = params
        .get("providerID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let provider = providers
        .into_iter()
        .find(|provider| provider.id == provider_id);
    match provider {
        Some(provider) => super::json(&crate::schema::LocationResponse {
            location: _location.info(),
            data: provider,
        }),
        None => {
            let message = format!("Provider not found: {provider_id}");
            Err(ApiError::ProviderNotFound {
                provider_id,
                message,
            })
        }
    }
}
