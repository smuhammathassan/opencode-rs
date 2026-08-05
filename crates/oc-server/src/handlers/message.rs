//! v2 message handler. From reference/packages/server/src/handlers/message.ts.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use base64::Engine;

use super::{json, HandlerResult};
use crate::errors::{session_not_found, ApiError};
use crate::schema::{SessionCursor, SessionMessagesResponse};

const DEFAULT_MESSAGES_LIMIT: usize = 50;
const BASE64_URL: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Message cursor: `base64url(JSON.stringify({ id, order, direction }))`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessageCursor {
    pub id: String,
    pub order: String,
    pub direction: String,
}

impl MessageCursor {
    fn encode(id: &str, order: &str, direction: &str) -> String {
        let payload = MessageCursor {
            id: id.into(),
            order: order.into(),
            direction: direction.into(),
        };
        let json = serde_json::to_string(&payload).unwrap_or_default();
        BASE64_URL.encode(json.as_bytes())
    }

    fn decode(input: &str) -> Result<MessageCursor, ApiError> {
        let decoded = BASE64_URL
            .decode(input.as_bytes())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| ApiError::InvalidCursor {
                message: "Invalid cursor".into(),
            })?;
        serde_json::from_str(&decoded).map_err(|_| ApiError::InvalidCursor {
            message: "Invalid cursor".into(),
        })
    }
}

/// `session.messages` from `reference/packages/server/src/handlers/message.ts`.
pub async fn session_messages(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;

    if let Some(cursor) = query.get("cursor") {
        if query.contains_key("order") {
            return Err(ApiError::InvalidCursor {
                message: "Cursor cannot be combined with order".into(),
            });
        }
        let _ = cursor;
    }
    let decoded = query
        .get("cursor")
        .map(|c| MessageCursor::decode(c))
        .transpose()?;
    let order = decoded
        .as_ref()
        .map(|c| c.order.clone())
        .or_else(|| query.get("order").cloned())
        .unwrap_or_else(|| "desc".into());
    let limit = query
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MESSAGES_LIMIT);

    let stores = state.stores.read().await;
    let record = stores
        .sessions
        .get(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    let mut messages = record.messages.clone();
    drop(stores);

    if order == "asc" {
        messages.sort_by_key(|m| {
            m.get("time")
                .and_then(|t| t.get("created"))
                .and_then(|c| c.as_i64())
        });
    } else {
        messages.sort_by_key(|m| {
            std::cmp::Reverse(
                m.get("time")
                    .and_then(|t| t.get("created"))
                    .and_then(|c| c.as_i64()),
            )
        });
    }

    // Paginate around the anchor: `next` continues in sort order after the anchor,
    // `previous` takes the items immediately before the anchor in sort order.
    let page: Vec<serde_json::Value> = match decoded {
        Some(cursor) => {
            let position = messages
                .iter()
                .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(cursor.id.as_str()));
            match position {
                Some(i) if cursor.direction == "next" => {
                    messages.into_iter().skip(i + 1).take(limit).collect()
                }
                Some(i) => messages
                    .into_iter()
                    .take(i)
                    .rev()
                    .take(limit)
                    .rev()
                    .collect(),
                None => Vec::new(),
            }
        }
        None => messages.into_iter().take(limit).collect(),
    };
    let first = page.first();
    let last = page.last();

    let cursor = SessionCursor {
        previous: first.map(|m| {
            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            MessageCursor::encode(id, &order, "previous")
        }),
        next: last.map(|m| {
            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            MessageCursor::encode(id, &order, "next")
        }),
    };

    json(&SessionMessagesResponse { data: page, cursor })
}
