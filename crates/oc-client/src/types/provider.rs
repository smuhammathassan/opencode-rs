//! Provider types.
//! From reference/packages/schema/src/provider.ts.
//!
//! Canonical home: `oc_schema::provider`.

use crate::types::location::LocationQueryRef;

// Re-export shim: `oc_schema::provider` is the single canonical definition.
pub use oc_schema::provider::{
    Api as ProviderApi, Info as ProviderInfo, Request as ProviderRequest,
};

/// `ProvidersGetInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProvidersGetInput {
    pub provider_id: String,
    pub location: Option<LocationQueryRef>,
}
