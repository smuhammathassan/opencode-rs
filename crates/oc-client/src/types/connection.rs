//! Connection types.
//! From reference/packages/schema/src/connection.ts.

/// `Connection.Info` — tagged on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ConnectionInfo {
    #[serde(rename = "credential")]
    Credential { id: String, label: String },
    #[serde(rename = "env")]
    Env { name: String },
}
