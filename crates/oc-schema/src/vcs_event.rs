//! From reference/packages/schema/src/vcs-event.ts

use crate::define_event;
use crate::event::Definition;

define_event! {
    /// `vcs.branch.updated`.
    pub struct BranchUpdated {
        tag: BranchUpdatedTag,
        r#type: "vcs.branch.updated",
        data: BranchUpdatedData,
    }
}

/// Payload of `vcs.branch.updated`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct BranchUpdatedData {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub branch: Option<String>,
}

/// `VcsEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[Definition {
    r#type: "vcs.branch.updated",
    durable: None,
}];
