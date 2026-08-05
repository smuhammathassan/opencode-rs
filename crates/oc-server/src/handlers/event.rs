//! v2 event handler. From reference/packages/server/src/handlers/event.ts.

use axum::extract::State;

use super::HandlerResult;
use crate::sse::v2_event_stream;

pub async fn event_subscribe(State(state): State<crate::state::AppState>) -> HandlerResult {
    Ok(v2_event_stream(state.events))
}
