//! Credential types.
//! From reference/packages/schema/src/credential.ts.
//!
//! Canonical home: `oc_schema::credential`.

use crate::types::location::LocationQueryRef;

// Re-export shim: `oc_schema::credential` is the single canonical definition.
pub use oc_schema::credential::Value as CredentialValue;

/// `CredentialsUpdateInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CredentialsUpdateInput {
    pub credential_id: String,
    pub location: Option<LocationQueryRef>,
    pub label: String,
}

/// `CredentialsRemoveInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CredentialsRemoveInput {
    pub credential_id: String,
    pub location: Option<LocationQueryRef>,
}
