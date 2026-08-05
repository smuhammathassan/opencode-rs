//! Health response type.
//! From reference/packages/protocol/src/groups/health.ts (`{ healthy: true }`).

/// The response of `health.get`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub healthy: bool,
}
