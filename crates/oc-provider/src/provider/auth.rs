//! ProviderAuth: provider auth methods, OAuth authorize/callback flow.
//!
//! From reference/packages/opencode/src/provider/auth.ts.
//!
//! The reference drives this from plugin-provided `auth` hooks
//! (`packages/plugin/src/index.ts`). The Rust port keeps the service logic and
//! data shapes and models the plugin hooks with the [`AuthHook`] trait so the
//! authorize/callback flow is testable without a plugin runtime.
//!
//! TODO(integration): wire `oc-plugin`'s hook discovery into [`ProviderAuth`]
//! so `methods()` reflects installed plugin auth hooks, and run hook
//! authorize/callback (async in the reference) on the crate's async runtime.

use std::cell::RefCell;
use std::collections::BTreeMap;

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

/// `ProviderAuthOauthCallbackFailed` from `auth.ts`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("OAuth callback failed")]
pub struct OauthCallbackFailed;

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
    fn authorize(&self, method_index: usize, inputs: &BTreeMap<String, String>) -> Result<AuthOAuthResult, anyhow::Error>;
    /// Completes an OAuth authorization with an optional code.
    fn callback(&self, code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error>;
}

/// `ProviderAuth.Service` from `auth.ts`.
pub struct ProviderAuth<H> {
    hooks: BTreeMap<String, H>,
    pending: RefCell<BTreeMap<String, AuthOAuthResult>>,
}

impl<H> ProviderAuth<H>
where
    H: AuthHook,
{
    pub fn new(hooks: BTreeMap<String, H>) -> Self {
        ProviderAuth {
            hooks,
            pending: RefCell::new(BTreeMap::new()),
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
        self.pending.borrow_mut().insert(provider_id.to_string(), result.clone());
        Ok(Some(Authorization {
            url: result.url,
            method: result.method,
            instructions: result.instructions,
        }))
    }

    /// `ProviderAuth.callback` from `auth.ts`.
    pub fn callback(
        &self,
        provider_id: &str,
        input: &CallbackInput,
        auth: &mut impl crate::auth::AuthStore,
    ) -> Result<(), ProviderAuthError> {
        let pending = self.pending.borrow();
        let Some(match_result) = pending.get(provider_id) else {
            return Err(ProviderAuthError::OauthMissing(OauthMissing {
                provider_id: provider_id.to_string(),
            }));
        };
        if match_result.method == CallbackMethod::Code && input.code.is_none() {
            return Err(ProviderAuthError::OauthCodeMissing(OauthCodeMissing {
                provider_id: provider_id.to_string(),
            }));
        }

        let hook = self
            .hooks
            .get(provider_id)
            .ok_or_else(|| ProviderAuthError::OauthMissing(OauthMissing {
                provider_id: provider_id.to_string(),
            }))?;
        let result = hook
            .callback(input.code.as_deref())
            .map_err(ProviderAuthError::Other)?;

        let AuthCallbackResult::Success { oauth, api, .. } = result else {
            return Err(ProviderAuthError::OauthCallbackFailed(OauthCallbackFailed));
        };

        if let Some(api) = api {
            auth.set(
                provider_id,
                crate::auth::Info::Api(crate::auth::Api {
                    key: api.key,
                    metadata: api.metadata,
                }),
            )?;
        }

        if let Some(oauth) = oauth {
            auth.set(
                provider_id,
                crate::auth::Info::Oauth(crate::auth::Oauth {
                    refresh: oauth.refresh,
                    access: oauth.access,
                    expires: oauth.expires,
                    account_id: oauth.account_id,
                    enterprise_url: oauth.enterprise_url,
                }),
            )?;
        }

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
    OauthCallbackFailed(#[from] OauthCallbackFailed),
    #[error(transparent)]
    ValidationFailed(#[from] ValidationFailed),
    #[error(transparent)]
    Auth(#[from] crate::auth::AuthError),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}
