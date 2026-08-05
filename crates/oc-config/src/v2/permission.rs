// From reference/packages/schema/src/permission.ts (`PermissionV2.Rule`/`Ruleset`)

use serde::{Deserialize, Serialize};

/// `PermissionV2.Effect` = `Schema.Literals(["allow", "deny", "ask"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
    Ask,
}

/// `PermissionV2.Rule`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub action: String,
    pub resource: String,
    pub effect: Effect,
}

/// `PermissionV2.Ruleset`.
pub type Ruleset = Vec<Rule>;
