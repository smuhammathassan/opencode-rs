//! Model types.
//! From reference/packages/schema/src/model.ts.
//!
//! Canonical home: `oc_schema::model`.

// Re-export shim: `oc_schema::model` is the single canonical definition.
pub use oc_schema::model::{
    Api as ModelApi, Capabilities as ModelCapabilities, Cost as ModelCost, CostTier as ModelTier,
    Info as ModelInfo, Limit as ModelLimit, Ref as ModelRef, Request as ModelRequest,
    Status as ModelStatus, Time as ModelTime, Variant as ModelVariant,
};
