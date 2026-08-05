//! Provider registry, ProviderTransform, auth and credentials.
//!
//! 1:1 Rust port of:
//! - `reference/packages/opencode/src/provider/` (provider.ts, transform.ts,
//!   model-status.ts, auth.ts, error.ts)
//! - `reference/packages/opencode/src/auth/` (credential storage + login flows)
//! - `reference/packages/core/src/credential.ts` (V2 credential abstraction)

pub mod auth;
pub mod credential;
pub mod models_dev;
pub mod provider;

pub use credential::{CredentialStore, Id as CredentialId, Value as CredentialValue};
