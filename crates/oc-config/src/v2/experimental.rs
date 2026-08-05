// From reference/packages/core/src/config/experimental.ts

use serde::{Deserialize, Serialize};

/// `Catalog.PolicyActions` = `Schema.Literals(["provider.use"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyAction {
    #[serde(rename = "provider.use")]
    ProviderUse,
}

/// `PolicyV2.Info` fields plus a constrained `action`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub action: PolicyAction,
    #[serde(rename = "effect")]
    pub effect: Effect,
    pub resource: String,
}

/// `Policy.Effect` = `Schema.Literals(["allow", "deny"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Experimental {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policies: Option<Vec<Policy>>,
}
