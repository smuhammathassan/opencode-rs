//! Session input types.
//! From reference/packages/schema/src/session-input.ts.

// TODO(integration): promote to oc-schema.
use crate::types::prompt::Prompt;
use crate::types::schema::{DateTimeMillis, Delivery};

/// `SessionInput.Admitted` — the result of `session.prompt`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInputAdmitted {
    pub admitted_seq: u64,
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub prompt: Prompt,
    pub delivery: Delivery,
    pub time_created: DateTimeMillis,
    #[serde(default)]
    pub promoted_seq: Option<u64>,
}
