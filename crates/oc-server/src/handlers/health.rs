//! Health handler. From reference/packages/server/src/handlers/health.ts.

use axum::extract::State;

use super::{json, HandlerResult};
use crate::schema::HealthOutput;

pub async fn health_get(State(_state): State<crate::state::AppState>) -> HandlerResult {
    json(&HealthOutput { healthy: true })
}
