/// From reference/packages/core/src/session/history.ts
///
/// Session history loading: context-epoch baseline + latest-compaction aware
/// message selection, newest-first decoding into V2 `Message`s.
use crate::v2::Message;

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub seq: u64,
    pub id: String,
    pub session_id: String,
    pub type_: String,
    pub data: serde_json::Value,
}

/// From reference `history.ts:messageRows` — select rows accounting for the
/// compaction boundary and the context-epoch baseline.
pub fn message_rows(
    rows: &[MessageRow],
    compaction: Option<u64>,
    baseline_seq: Option<u64>,
) -> Vec<MessageRow> {
    let mut result: Vec<MessageRow> = Vec::new();
    for row in rows {
        let after_compaction = match compaction {
            Some(compaction) => {
                row.seq >= compaction
                    || match baseline_seq {
                        Some(baseline) => row.type_ == "system" && row.seq > baseline,
                        None => false,
                    }
            }
            None => true,
        };
        let baseline_ok = match baseline_seq {
            Some(baseline) => row.type_ != "system" || row.seq > baseline,
            None => true,
        };
        if after_compaction && baseline_ok {
            result.push(row.clone());
        }
    }
    result.sort_by_key(|row| row.seq);
    result
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("Failed to decode message {message_id} in session {session_id}")]
pub struct MessageDecodeError {
    pub session_id: String,
    pub message_id: String,
}

/// From reference `history.ts:load`.
pub fn load(
    db: &dyn crate::store::SessionDb,
    session_id: &str,
) -> Vec<Result<Message, MessageDecodeError>> {
    let epoch = db.context_epoch_baseline(session_id);
    let compaction = db.latest_compaction_seq(session_id);
    let rows = db.message_rows(session_id);
    message_rows(&rows, compaction, epoch)
        .into_iter()
        .map(decode_row)
        .collect()
}

/// From reference `history.ts:loadForRunner`.
pub fn load_for_runner(
    db: &dyn crate::store::SessionDb,
    session_id: &str,
    baseline_seq: u64,
) -> Vec<Result<Message, MessageDecodeError>> {
    let compaction = db.latest_compaction_seq(session_id);
    let rows = db.message_rows(session_id);
    message_rows(&rows, compaction, Some(baseline_seq))
        .into_iter()
        .map(decode_row)
        .collect()
}

pub fn decode_row(row: MessageRow) -> Result<Message, MessageDecodeError> {
    let mut value = row.data.clone();
    if let serde_json::Value::Object(map) = &mut value {
        map.insert("id".to_string(), serde_json::Value::String(row.id.clone()));
        map.insert(
            "type".to_string(),
            serde_json::Value::String(row.type_.clone()),
        );
    }
    serde_json::from_value(value).map_err(|_| MessageDecodeError {
        session_id: row.session_id,
        message_id: row.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(seq: u64, id: &str, type_: &str) -> MessageRow {
        MessageRow {
            seq,
            id: id.into(),
            session_id: "ses1".into(),
            type_: type_.into(),
            data: serde_json::json!({ "text": "x", "time": { "created": 1 } }),
        }
    }

    #[test]
    fn message_rows_filter_by_compaction_and_baseline() {
        let rows = vec![
            row(1, "m1", "user"),
            row(2, "m2", "assistant"),
            row(3, "m3", "compaction"),
            row(4, "m4", "user"),
            row(5, "m5", "system"),
        ];
        // after compaction (seq 3), plus system rows after baseline (0)
        let selected = message_rows(&rows, Some(3), Some(0));
        let ids: Vec<&str> = selected.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["m3", "m4", "m5"]);
    }

    #[test]
    fn decode_row_reinjects_id_and_type() {
        let row = row(1, "msg_1", "user");
        let message = decode_row(row).unwrap();
        assert_eq!(message.id(), "msg_1");
    }
}
