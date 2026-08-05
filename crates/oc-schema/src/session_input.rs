//! From reference/packages/schema/src/session-input.ts

use crate::prompt::Prompt;
use crate::schema::{DateTimeUtc, NonNegativeInt};
use crate::session_id::SessionID;
use crate::session_message;
use serde::{Deserialize, Serialize};

/// `SessionInput.Delivery`.
pub use crate::session_delivery::Delivery;

/// `SessionInput.Admitted`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Admitted {
    #[serde(rename = "admittedSeq")]
    pub admitted_seq: NonNegativeInt,
    pub id: session_message::ID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub prompt: Prompt,
    pub delivery: Delivery,
    #[serde(rename = "timeCreated")]
    pub time_created: DateTimeUtc,
    #[serde(rename = "promotedSeq", skip_serializing_if = "Option::is_none", default)]
    pub promoted_seq: Option<NonNegativeInt>,
}
