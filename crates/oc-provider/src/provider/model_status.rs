//! From reference/packages/opencode/src/provider/model-status.ts

use serde::{Deserialize, Serialize};

/// Lifecycle status of a provider model.
///
/// Mirrors `ModelStatus` in `provider/model-status.ts` which unions the catalog
/// status (`alpha | beta | deprecated`) with the `active` status used once a
/// model is loaded into the provider registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Alpha,
    Beta,
    Deprecated,
    Active,
}

/// Lifecycle status advertised by the models.dev catalog.
///
/// Mirrors `CatalogModelStatus` in `@opencode-ai/core/models-dev`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogModelStatus {
    Alpha,
    Beta,
    Deprecated,
}
