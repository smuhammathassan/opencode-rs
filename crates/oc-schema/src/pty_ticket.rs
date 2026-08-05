//! From reference/packages/schema/src/pty-ticket.ts

use crate::schema::PositiveInt;
use serde::{Deserialize, Serialize};

/// `PtyTicket.ConnectToken`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConnectToken {
    pub ticket: String,
    pub expires_in: PositiveInt,
}
