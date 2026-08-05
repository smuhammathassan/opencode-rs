//! From reference/packages/schema/src/session-delivery.ts

use serde::{Deserialize, Serialize};

/// `SessionDelivery.Delivery`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "queue")]
    Queue,
}
