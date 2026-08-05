//! Provider auth-option helpers.
//! From reference/packages/llm/src/route/auth-options.ts

use super::auth::{optional, Auth, Credential};

/// Standard bearer-auth resolution for providers: honor an explicit `auth`
/// override, otherwise resolve `apiKey` (option > env var) and apply it as a
/// bearer token.
///
/// `AuthOptions.bearer(options, envVar)`.
/// From reference/packages/llm/src/route/auth-options.ts
pub fn bearer(auth: Option<Auth>, api_key: Option<String>, env_var: &[&str]) -> Auth {
    if let Some(auth) = auth {
        return auth;
    }
    let mut credential = optional(api_key, "apiKey");
    for name in env_var {
        credential = credential.or_else(Credential::Env(name.to_string()));
    }
    credential.bearer_auth()
}

/// `AtLeastOne<T>` shapes are modeled structurally at call sites; this is a
/// marker doc for the reference type.
/// From reference/packages/llm/src/route/auth-options.ts
pub type AtLeastOne<T> = T;
