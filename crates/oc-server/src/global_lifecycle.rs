//! Global lifecycle. From reference/packages/opencode/src/server/global-lifecycle.ts.

use crate::event::{event_id, Event};
use crate::state::AppState;

/// Emit the `global.disposed` event. From reference/packages/opencode/src/server/
/// global-lifecycle.ts (`emitGlobalDisposed`).
pub fn emit_global_disposed(state: &AppState) {
    state.events.emit(Event {
        id: event_id(),
        metadata: None,
        r#type: "global.disposed".into(),
        durable: None,
        location: None,
        data: serde_json::json!({}),
    });
}

/// Dispose all instances and emit the global disposed event. From
/// reference/packages/opencode/src/server/global-lifecycle.ts
/// (`disposeAllInstancesAndEmitGlobalDisposed`).
pub async fn dispose_all_instances_and_emit_global_disposed(state: &AppState) {
    let mut stores = state.stores.write().await;
    for record in stores.sessions.values_mut() {
        record.active = false;
    }
    drop(stores);
    emit_global_disposed(state);
}
