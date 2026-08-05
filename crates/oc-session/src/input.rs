/// From reference/packages/core/src/session/input.ts and
/// reference/packages/schema/src/session-input.ts
///
/// Admitted prompt inputs (steer/queue delivery) and their equivalence
/// checks. The DB-backed admit/promote projections are provided by the store.
use serde::{Deserialize, Serialize};

use crate::v2::Prompt;

/// `SessionDelivery.Delivery` — `"steer" | "queue"`.
pub type Delivery = String;

pub const DELIVERY_STEER: &str = "steer";
pub const DELIVERY_QUEUE: &str = "queue";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Admitted {
    pub admitted_seq: u64,
    pub id: String,
    pub session_id: String,
    pub prompt: Prompt,
    pub delivery: Delivery,
    pub time_created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promoted_seq: Option<u64>,
}

/// From reference `input.ts:equivalent`.
pub fn equivalent(input: &Admitted, expected: &AdmittedProjection) -> bool {
    input.delivery == expected.delivery && matches_prompt(input, expected)
}

/// From reference `input.ts:matchesPrompt` — compares the encoded prompt JSON.
pub fn matches_prompt(input: &Admitted, expected: &AdmittedProjection) -> bool {
    input.session_id == expected.session_id
        && serde_json::to_string(&input.prompt).ok() == serde_json::to_string(&expected.prompt).ok()
}

#[derive(Debug, Clone)]
pub struct AdmittedProjection {
    pub session_id: String,
    pub prompt: Prompt,
    pub delivery: Delivery,
    pub time_created: u64,
}

/// From reference `input.ts:matchesProjection`.
pub fn matches_projection(input: &Admitted, expected: &AdmittedProjection) -> bool {
    equivalent(input, expected) && input.time_created == expected.time_created
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str) -> Prompt {
        Prompt {
            text: text.into(),
            files: None,
            agents: None,
        }
    }

    #[test]
    fn equivalent_compares_prompt_json_and_delivery() {
        let admitted = Admitted {
            admitted_seq: 1,
            id: "msg_1".into(),
            session_id: "ses1".into(),
            prompt: prompt("hi"),
            delivery: DELIVERY_STEER.into(),
            time_created: 1000,
            promoted_seq: None,
        };
        assert!(equivalent(
            &admitted,
            &AdmittedProjection {
                session_id: "ses1".into(),
                prompt: prompt("hi"),
                delivery: DELIVERY_STEER.into(),
                time_created: 1000,
            }
        ));
        assert!(!equivalent(
            &admitted,
            &AdmittedProjection {
                session_id: "ses1".into(),
                prompt: prompt("bye"),
                delivery: DELIVERY_STEER.into(),
                time_created: 1000,
            }
        ));
    }

    #[test]
    fn admitted_serializes_with_promoted_seq_omitted() {
        let admitted = Admitted {
            admitted_seq: 1,
            id: "msg_1".into(),
            session_id: "ses1".into(),
            prompt: prompt("hi"),
            delivery: DELIVERY_STEER.into(),
            time_created: 1000,
            promoted_seq: None,
        };
        let value = serde_json::to_value(&admitted).unwrap();
        assert!(value.get("promotedSeq").is_none());
        assert_eq!(value["admittedSeq"], 1);
    }
}
