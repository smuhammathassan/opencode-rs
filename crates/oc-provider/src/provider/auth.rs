//! ProviderAuth: provider auth methods, OAuth authorize/callback flow.
//!
//! From reference/packages/opencode/src/provider/auth.ts.
//!
//! The reference drives this from plugin-provided `auth` hooks
//! (`packages/plugin/src/index.ts`). The Rust port keeps the service logic and
//! data shapes and models the plugin hooks with the [`AuthHook`] trait so the
//! authorize/callback flow is testable without a plugin runtime.
//!
//! Plugin discovery is intentionally outside this crate. Hosts can provide
//! hooks through [`ProviderAuth::new`]; an empty hook map is an honest
//! unsupported-auth state rather than a fabricated OAuth implementation.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// `When` rule from `auth.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct When {
    pub key: String,
    #[serde(rename = "op")]
    pub op: WhenOp,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhenOp {
    Eq,
    Neq,
}

/// `TextPrompt` from `auth.ts`. `validate` is plugin-side and not serialized.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPrompt {
    #[serde(rename = "type")]
    pub r#type: String,
    pub key: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

/// `SelectOption` from `auth.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `SelectPrompt` from `auth.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectPrompt {
    #[serde(rename = "type")]
    pub r#type: String,
    pub key: String,
    pub message: String,
    pub options: Vec<SelectOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

/// `Prompt` from `auth.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Prompt {
    Text(TextPrompt),
    Select(SelectPrompt),
}

impl Prompt {
    pub fn key(&self) -> &str {
        match self {
            Prompt::Text(prompt) => &prompt.key,
            Prompt::Select(prompt) => &prompt.key,
        }
    }
}

/// `Method` from `auth.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Method {
    #[serde(rename = "type")]
    pub r#type: MethodType,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<Prompt>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MethodType {
    OAuth,
    Api,
}

/// `Methods` from `auth.ts`: `Record<ProviderID, Method[]>`.
pub type Methods = BTreeMap<String, Vec<Method>>;

/// `Authorization` from `auth.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
    pub url: String,
    #[serde(rename = "method")]
    pub method: CallbackMethod,
    pub instructions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallbackMethod {
    Auto,
    Code,
}

/// `AuthorizeInput` from `auth.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeInput {
    pub method: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<BTreeMap<String, String>>,
}

/// `CallbackInput` from `auth.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackInput {
    pub method: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `ProviderAuthValidationFailed` from `auth.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("Validation failed: {message} ({field})")]
pub struct ValidationFailed {
    pub field: String,
    pub message: String,
}

/// `ProviderAuthOauthMissing` from `auth.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("ProviderAuthOauthMissing: {provider_id}")]
pub struct OauthMissing {
    pub provider_id: String,
}

/// `ProviderAuthOauthCodeMissing` from `auth.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("ProviderAuthOauthCodeMissing: {provider_id}")]
pub struct OauthCodeMissing {
    pub provider_id: String,
}

/// Callback method did not match the pending authorization request.
#[derive(Debug, Clone, thiserror::Error)]
#[error("ProviderAuthOauthMethodMismatch: {provider_id}")]
pub struct OauthMethodMismatch {
    pub provider_id: String,
}

/// `ProviderAuthOauthCallbackFailed` from `auth.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("OAuth callback failed")]
pub struct OauthCallbackFailed;

/// A refresh hook returned a token without an access token.
#[derive(Debug, Clone, thiserror::Error)]
#[error("ProviderAuthOauthRefreshInvalid: {provider_id}")]
pub struct OauthRefreshInvalid {
    pub provider_id: String,
}

/// The `AuthOAuthResult` returned by a plugin `authorize()`.
///
/// From `packages/plugin/src/index.ts`. The callback is invoked with the
/// authorization code for `Code` methods.
#[derive(Debug, Clone)]
pub struct AuthOAuthResult {
    pub url: String,
    pub method: CallbackMethod,
    pub instructions: String,
}

/// A successful OAuth/API authorization result.
///
/// Mirrors the success payloads of `AuthOAuthResult`'s callback and the `api`
/// method's `authorize`.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthCallbackResult {
    Success {
        /// Optional provider override under which to store the credential.
        provider: Option<String>,
        /// OAuth credential (`refresh`/`access`/`expires`).
        oauth: Option<OAuthCredential>,
        /// API-key credential (`key`/`metadata`).
        api: Option<ApiCredential>,
    },
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OAuthCredential {
    pub refresh: String,
    pub access: String,
    pub expires: u64,
    pub account_id: Option<String>,
    pub enterprise_url: Option<String>,
}

/// Result of checking an OAuth credential for refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OauthRefreshResult {
    /// The credential is still valid, or is not an OAuth credential.
    NotNeeded,
    /// The credential is expired, but no refresh hook/token is available.
    Unsupported,
    /// The hook returned a replacement and it was persisted.
    Refreshed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiCredential {
    pub key: String,
    pub metadata: Option<BTreeMap<String, String>>,
}

/// Validation closure for a text prompt, mirroring the plugin `validate` hook.
pub type ValidateFn = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// A plugin-provided auth hook (mirrors `AuthHook` from the plugin package).
pub trait AuthHook: Send + Sync {
    /// The auth methods this hook advertises, in order.
    fn methods(&self) -> Vec<Method>;
    /// Validates a text-prompt input for `method_index`/`prompt_key`.
    fn validate(&self, method_index: usize, key: &str, value: &str) -> Option<String>;
    /// Starts an OAuth or API authorization flow.
    fn authorize(
        &self,
        method_index: usize,
        inputs: &BTreeMap<String, String>,
    ) -> Result<AuthOAuthResult, anyhow::Error>;
    /// Completes an OAuth authorization with an optional code.
    fn callback(&self, code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error>;

    /// Refreshes an expired OAuth credential when the host/plugin owns that
    /// capability. The default is deliberately unsupported: this crate does
    /// not invent a token endpoint or perform network I/O.
    fn refresh(
        &self,
        _credential: &crate::auth::Oauth,
    ) -> Result<Option<OAuthCredential>, anyhow::Error> {
        Ok(None)
    }
}

impl<T> AuthHook for Box<T>
where
    T: AuthHook + ?Sized,
{
    fn methods(&self) -> Vec<Method> {
        (**self).methods()
    }

    fn validate(&self, method_index: usize, key: &str, value: &str) -> Option<String> {
        (**self).validate(method_index, key, value)
    }

    fn authorize(
        &self,
        method_index: usize,
        inputs: &BTreeMap<String, String>,
    ) -> Result<AuthOAuthResult, anyhow::Error> {
        (**self).authorize(method_index, inputs)
    }

    fn callback(&self, code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error> {
        (**self).callback(code)
    }

    fn refresh(
        &self,
        credential: &crate::auth::Oauth,
    ) -> Result<Option<OAuthCredential>, anyhow::Error> {
        (**self).refresh(credential)
    }
}

/// The host-facing registry shape used by the server when it has dynamic
/// hooks. Hosts that do not load plugins should leave it empty.
pub type BuiltinProviderAuth = ProviderAuth<Box<dyn AuthHook>>;

#[derive(Debug, Clone)]
struct PendingAuthorization {
    method: usize,
    result: AuthOAuthResult,
}

/// `ProviderAuth.Service` from `auth.ts`.
pub struct ProviderAuth<H> {
    hooks: BTreeMap<String, H>,
    pending: Mutex<BTreeMap<String, PendingAuthorization>>,
}

impl<H> ProviderAuth<H>
where
    H: AuthHook,
{
    pub fn new(hooks: BTreeMap<String, H>) -> Self {
        ProviderAuth {
            hooks,
            pending: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn method(&self, provider_id: &str, index: usize) -> Option<Method> {
        self.hooks
            .get(provider_id)
            .and_then(|hook| hook.methods().into_iter().nth(index))
    }

    /// `ProviderAuth.methods` from `auth.ts`.
    pub fn methods(&self) -> Methods {
        self.hooks
            .iter()
            .map(|(provider_id, hook)| (provider_id.clone(), hook.methods()))
            .collect()
    }

    /// Cancels the pending authorization for a provider, if any.
    ///
    /// Integration attempts have their own public IDs, so cancellation is
    /// deliberately best-effort at this lower layer and does not error when
    /// a callback already consumed the pending authorization.
    pub fn cancel(&self, provider_id: &str) {
        self.pending
            .lock()
            .expect("provider auth pending lock poisoned")
            .remove(provider_id);
    }

    /// `ProviderAuth.authorize` from `auth.ts`.
    pub fn authorize(
        &self,
        provider_id: &str,
        input: &AuthorizeInput,
    ) -> Result<Option<Authorization>, ProviderAuthError> {
        let Some(hook) = self.hooks.get(provider_id) else {
            return Err(ProviderAuthError::OauthMissing(OauthMissing {
                provider_id: provider_id.to_string(),
            }));
        };
        let methods = hook.methods();
        let Some(method) = methods.get(input.method) else {
            return Ok(None);
        };
        if method.r#type != MethodType::OAuth {
            return Ok(None);
        }

        if let Some(prompts) = &method.prompts {
            if let Some(inputs) = &input.inputs {
                for prompt in prompts {
                    let Prompt::Text(text_prompt) = prompt else {
                        continue;
                    };
                    if let Some(value) = inputs.get(&text_prompt.key) {
                        if let Some(error) = hook.validate(input.method, &text_prompt.key, value) {
                            return Err(ProviderAuthError::ValidationFailed(ValidationFailed {
                                field: text_prompt.key.clone(),
                                message: error,
                            }));
                        }
                    }
                }
            }
        }

        let inputs = input.inputs.clone().unwrap_or_default();
        let result = hook
            .authorize(input.method, &inputs)
            .map_err(ProviderAuthError::Other)?;
        self.pending
            .lock()
            .expect("provider auth pending lock poisoned")
            .insert(
                provider_id.to_string(),
                PendingAuthorization {
                    method: input.method,
                    result: result.clone(),
                },
            );
        Ok(Some(Authorization {
            url: result.url,
            method: result.method,
            instructions: result.instructions,
        }))
    }

    /// Refreshes an expired persisted OAuth credential through the optional
    /// host/plugin hook and replaces it through [`AuthStore::set`].
    ///
    /// The timestamp is supplied by the caller so this method remains
    /// deterministic and transport-agnostic. A refresh hook may rotate the
    /// refresh token; when it omits one, the existing refresh token is kept.
    pub fn refresh(
        &self,
        provider_id: &str,
        now_ms: u64,
        auth: &mut impl crate::auth::AuthStore,
    ) -> Result<OauthRefreshResult, ProviderAuthError> {
        let Some(info) = auth.get(provider_id)? else {
            return Err(ProviderAuthError::OauthMissing(OauthMissing {
                provider_id: provider_id.to_string(),
            }));
        };
        let crate::auth::Info::Oauth(current) = info else {
            return Ok(OauthRefreshResult::NotNeeded);
        };
        if !current.is_expired_at(now_ms) {
            return Ok(OauthRefreshResult::NotNeeded);
        }
        if current.refresh.is_empty() {
            return Ok(OauthRefreshResult::Unsupported);
        }
        let Some(hook) = self.hooks.get(provider_id) else {
            return Ok(OauthRefreshResult::Unsupported);
        };
        let Some(refreshed) = hook.refresh(&current).map_err(ProviderAuthError::Other)? else {
            return Ok(OauthRefreshResult::Unsupported);
        };
        if refreshed.access.is_empty() {
            return Err(ProviderAuthError::OauthRefreshInvalid(
                OauthRefreshInvalid {
                    provider_id: provider_id.to_string(),
                },
            ));
        }

        let rotated = crate::auth::Oauth {
            refresh: if refreshed.refresh.is_empty() {
                current.refresh
            } else {
                refreshed.refresh
            },
            access: refreshed.access,
            expires: refreshed.expires,
            account_id: refreshed.account_id.or(current.account_id),
            enterprise_url: refreshed.enterprise_url.or(current.enterprise_url),
        };
        auth.set(provider_id, crate::auth::Info::Oauth(rotated))?;
        Ok(OauthRefreshResult::Refreshed)
    }

    /// `ProviderAuth.callback` from `auth.ts`.
    pub fn callback(
        &self,
        provider_id: &str,
        input: &CallbackInput,
        auth: &mut impl crate::auth::AuthStore,
    ) -> Result<(), ProviderAuthError> {
        let pending = self
            .pending
            .lock()
            .expect("provider auth pending lock poisoned")
            .get(provider_id)
            .cloned();
        let Some(pending) = pending else {
            return Err(ProviderAuthError::OauthMissing(OauthMissing {
                provider_id: provider_id.to_string(),
            }));
        };
        if pending.method != input.method {
            return Err(ProviderAuthError::OauthMethodMismatch(
                OauthMethodMismatch {
                    provider_id: provider_id.to_string(),
                },
            ));
        }
        let code = input.code.as_deref();
        if pending.result.method == CallbackMethod::Code && code.map_or(true, str::is_empty) {
            return Err(ProviderAuthError::OauthCodeMissing(OauthCodeMissing {
                provider_id: provider_id.to_string(),
            }));
        }

        let hook = self.hooks.get(provider_id).ok_or_else(|| {
            ProviderAuthError::OauthMissing(OauthMissing {
                provider_id: provider_id.to_string(),
            })
        })?;
        // Match the plugin contract: only code-based flows receive the
        // submitted code. Auto callbacks are invoked without an argument,
        // even if a client included one in the request.
        let result = hook
            .callback(match pending.result.method {
                CallbackMethod::Auto => None,
                CallbackMethod::Code => code,
            })
            .map_err(ProviderAuthError::Other)?;

        let AuthCallbackResult::Success {
            provider,
            oauth,
            api,
        } = result
        else {
            return Err(ProviderAuthError::OauthCallbackFailed(OauthCallbackFailed));
        };

        let credential_provider = provider.unwrap_or_else(|| provider_id.to_string());

        if let Some(api) = api {
            auth.set(
                &credential_provider,
                crate::auth::Info::Api(crate::auth::Api {
                    key: api.key,
                    metadata: api.metadata,
                }),
            )?;
        }

        if let Some(oauth) = oauth {
            auth.set(
                &credential_provider,
                crate::auth::Info::Oauth(crate::auth::Oauth {
                    refresh: oauth.refresh,
                    access: oauth.access,
                    expires: oauth.expires,
                    account_id: oauth.account_id,
                    enterprise_url: oauth.enterprise_url,
                }),
            )?;
        }

        self.pending
            .lock()
            .expect("provider auth pending lock poisoned")
            .remove(provider_id);

        Ok(())
    }
}

/// Errors surfaced by [`ProviderAuth`].
#[derive(Debug, thiserror::Error)]
pub enum ProviderAuthError {
    #[error(transparent)]
    OauthMissing(#[from] OauthMissing),
    #[error(transparent)]
    OauthCodeMissing(#[from] OauthCodeMissing),
    #[error(transparent)]
    OauthMethodMismatch(#[from] OauthMethodMismatch),
    #[error(transparent)]
    OauthCallbackFailed(#[from] OauthCallbackFailed),
    #[error(transparent)]
    OauthRefreshInvalid(#[from] OauthRefreshInvalid),
    #[error(transparent)]
    ValidationFailed(#[from] ValidationFailed),
    #[error(transparent)]
    Auth(#[from] crate::auth::AuthError),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}
