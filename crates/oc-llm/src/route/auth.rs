//! Transport authentication.
//! From reference/packages/llm/src/route/auth.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::schema::{AuthKind, LlmError, LlmErrorReason};

pub type HeaderMap = BTreeMap<String, String>;

/// `MissingCredentialError`.
/// From reference/packages/llm/src/route/auth.ts
#[derive(Debug, Clone)]
pub struct MissingCredentialError {
    pub source: String,
}

impl MissingCredentialError {
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into() }
    }
}

impl std::fmt::Display for MissingCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Missing auth credential: {}", self.source)
    }
}

impl std::error::Error for MissingCredentialError {}

pub type CredentialError = MissingCredentialError;

/// `AuthInput`.
/// From reference/packages/llm/src/route/auth.ts
#[derive(Debug, Clone)]
pub struct AuthInput {
    pub request: crate::schema::LlmRequest,
    pub method: String,
    pub url: String,
    pub body: String,
    pub headers: HeaderMap,
}

/// `Credential` — a lazily-loaded secret.
/// From reference/packages/llm/src/route/auth.ts
#[derive(Debug, Clone)]
pub enum Credential {
    /// Already-resolved secret value.
    Value(String),
    /// Fails with `MissingCredentialError(source)`.
    Missing(String),
    /// Resolves from an environment variable at load time.
    Env(String),
    /// Try `self`, then `that`.
    OrElse(Arc<Credential>, Arc<Credential>),
}

impl Credential {
    pub fn load(&self) -> Result<String, CredentialError> {
        match self {
            Credential::Value(value) => {
                if value.is_empty() {
                    Err(MissingCredentialError::new("value"))
                } else {
                    Ok(value.clone())
                }
            }
            Credential::Missing(source) => Err(MissingCredentialError::new(source)),
            Credential::Env(name) => match std::env::var(name) {
                Ok(value) if !value.is_empty() => Ok(value),
                _ => Err(MissingCredentialError::new(name)),
            },
            Credential::OrElse(a, b) => match a.load() {
                Ok(value) => Ok(value),
                Err(_) => b.load(),
            },
        }
    }

    pub fn or_else(&self, that: Credential) -> Credential {
        Credential::OrElse(Arc::new(self.clone()), Arc::new(that))
    }
}

/// How a resolved secret is rendered into a header.
#[derive(Debug, Clone)]
pub enum HeaderRender {
    Bearer,
    Header(String),
    BearerHeader(String),
}

/// `Auth` — per-request header mutation.
/// From reference/packages/llm/src/route/auth.ts
#[derive(Clone)]
pub enum Auth {
    None,
    /// Fixed headers merged onto the input.
    Headers(HeaderMap),
    /// Remove the named header.
    Remove(String),
    /// Render a credential into a header.
    Credential {
        credential: Credential,
        render: HeaderRender,
    },
    /// Apply `self`, then `that` on the merged result.
    AndThen(Box<Auth>, Box<Auth>),
    /// Apply `self`, falling back to `that` on failure.
    OrElse(Box<Auth>, Box<Auth>),
    /// Arbitrary header logic.
    Custom(Arc<dyn Fn(&AuthInput) -> Result<HeaderMap, LlmError> + Send + Sync>),
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::None => f.write_str("Auth::None"),
            Auth::Headers(_) => f.write_str("Auth::Headers"),
            Auth::Remove(name) => write!(f, "Auth::Remove({name})"),
            Auth::Credential { render, .. } => write!(f, "Auth::Credential({render:?})"),
            Auth::AndThen(_, _) => f.write_str("Auth::AndThen"),
            Auth::OrElse(_, _) => f.write_str("Auth::OrElse"),
            Auth::Custom(_) => f.write_str("Auth::Custom"),
        }
    }
}

impl Auth {
    pub fn apply(&self, input: &AuthInput) -> Result<HeaderMap, LlmError> {
        match self {
            Auth::None => Ok(input.headers.clone()),
            Auth::Headers(headers) => {
                let mut merged = input.headers.clone();
                for (name, value) in headers {
                    merged.insert(name.clone(), value.clone());
                }
                Ok(merged)
            }
            Auth::Remove(name) => {
                let mut headers = input.headers.clone();
                let mut to_remove = None;
                for key in headers.keys() {
                    if key.eq_ignore_ascii_case(name) {
                        to_remove = Some(key.clone());
                        break;
                    }
                }
                if let Some(key) = to_remove {
                    headers.remove(&key);
                }
                Ok(headers)
            }
            Auth::Credential { credential, render } => {
                let value = credential
                    .load()
                    .map_err(|error| to_llm_error(&error))?;
                let mut headers = input.headers.clone();
                match render {
                    HeaderRender::Bearer => {
                        headers.insert("authorization".to_string(), format!("Bearer {}", value));
                    }
                    HeaderRender::Header(name) => {
                        headers.insert(name.clone(), value);
                    }
                    HeaderRender::BearerHeader(name) => {
                        headers.insert(name.clone(), format!("Bearer {}", value));
                    }
                }
                Ok(headers)
            }
            Auth::AndThen(a, b) => {
                let merged = a.apply(input)?;
                let next = AuthInput { headers: merged, ..input.clone() };
                b.apply(&next)
            }
            Auth::OrElse(a, b) => match a.apply(input) {
                Ok(headers) => Ok(headers),
                Err(_) => b.apply(input),
            },
            Auth::Custom(apply) => apply(input),
        }
    }
}

fn to_llm_error(error: &CredentialError) -> LlmError {
    LlmError::new(
        "Auth",
        "apply",
        LlmErrorReason::Authentication {
            message: error.to_string(),
            kind: AuthKind::Missing,
            provider_metadata: None,
            http: None,
        },
    )
}

fn from_credential(credential: Credential, render: HeaderRender) -> Auth {
    Auth::Credential { credential, render }
}

fn credential_input(source: Credential) -> Credential {
    source
}

/// `Auth.none`.
pub fn none() -> Auth {
    Auth::None
}

/// `Auth.headers(input)`.
pub fn headers(input: HeaderMap) -> Auth {
    Auth::Headers(input)
}

/// `Auth.remove(name)`.
pub fn remove(name: impl Into<String>) -> Auth {
    Auth::Remove(name.into())
}

/// `Auth.custom(apply)`.
pub fn custom(apply: impl Fn(&AuthInput) -> Result<HeaderMap, LlmError> + Send + Sync + 'static) -> Auth {
    Auth::Custom(Arc::new(apply))
}

/// `Auth.passthrough` — no-op auth.
pub fn passthrough() -> Auth {
    Auth::None
}

/// `Auth.value(secret, source)`.
pub fn value(secret: impl Into<String>) -> Credential {
    Credential::Value(secret.into())
}

/// `Auth.optional(secret, source)` — fails when `secret` is absent.
pub fn optional(secret: Option<String>, source: &str) -> Credential {
    match secret {
        Some(value) => Credential::Value(value),
        None => Credential::Missing(source.to_string()),
    }
}

/// `Auth.config(name)` — resolves an env var at load time.
pub fn config(name: impl Into<String>) -> Credential {
    Credential::Env(name.into())
}

/// `Auth.bearer(source)`.
pub fn bearer(source: Credential) -> Auth {
    from_credential(credential_input(source), HeaderRender::Bearer)
}

/// `Auth.apiKey` — alias of `bearer`.
pub fn api_key(source: Credential) -> Auth {
    bearer(source)
}

/// `Auth.header(name, source)`.
pub fn header(name: impl Into<String>, source: Credential) -> Auth {
    from_credential(credential_input(source), HeaderRender::Header(name.into()))
}

/// `Auth.bearerHeader(name, source)`.
pub fn bearer_header(name: impl Into<String>, source: Credential) -> Auth {
    from_credential(credential_input(source), HeaderRender::BearerHeader(name.into()))
}

impl Credential {
    /// Render the credential as a bearer token.
    pub fn bearer_auth(&self) -> Auth {
        from_credential(self.clone(), HeaderRender::Bearer)
    }

    /// Render the credential into a named header.
    pub fn header_auth(&self, name: impl Into<String>) -> Auth {
        from_credential(self.clone(), HeaderRender::Header(name.into()))
    }
}

/// Credential combinators on `Auth` for ergonomic chaining.
impl Auth {
    /// `auth.andThen(that)`.
    pub fn and_then(self, that: Auth) -> Auth {
        Auth::AndThen(Box::new(self), Box::new(that))
    }

    /// `auth.orElse(that)`.
    pub fn or_else(self, that: Auth) -> Auth {
        Auth::OrElse(Box::new(self), Box::new(that))
    }

    /// `auth.pipe(f)` — `f(self)`.
    pub fn pipe<A>(self, f: impl FnOnce(Auth) -> A) -> A {
        f(self)
    }
}

impl Auth {
    /// `Auth.none`.
    pub fn none() -> Auth {
        none()
    }

    /// `Auth.passthrough`.
    pub fn passthrough() -> Auth {
        none()
    }

    /// `Auth.headers(input)`.
    pub fn headers(input: HeaderMap) -> Auth {
        headers(input)
    }

    /// `Auth.remove(name)`.
    pub fn remove(name: impl Into<String>) -> Auth {
        remove(name)
    }

    /// `Auth.custom(apply)`.
    pub fn custom(apply: impl Fn(&AuthInput) -> Result<HeaderMap, LlmError> + Send + Sync + 'static) -> Auth {
        custom(apply)
    }

    /// `Auth.bearer(source)`.
    pub fn bearer(source: Credential) -> Auth {
        bearer(source)
    }

    /// `Auth.apiKey` — alias of `bearer`.
    pub fn api_key(source: Credential) -> Auth {
        api_key(source)
    }

    /// `Auth.header(name, source)`.
    pub fn header(name: impl Into<String>, source: Credential) -> Auth {
        header(name, source)
    }

    /// `Auth.bearerHeader(name, source)`.
    pub fn bearer_header(name: impl Into<String>, source: Credential) -> Auth {
        bearer_header(name, source)
    }

    /// `Auth.optional(secret, source)` — a credential, not an auth.
    pub fn optional(secret: Option<String>, source: &str) -> Credential {
        optional(secret, source)
    }

    /// `Auth.config(name)` — a credential resolving an env var.
    pub fn config(name: impl Into<String>) -> Credential {
        config(name)
    }

    /// `Auth.value(secret)` — a credential from a literal value.
    pub fn value(secret: impl Into<String>) -> Credential {
        value(secret)
    }
}
