//! The event-sourcing store: durable event commit, replay, and cursor ordering.
//!
//! Ports the `EventV2` layer in reference/packages/core/src/event.ts: the total
//! order for a session aggregate is a per-aggregate monotonic `seq` starting at
//! `-1` for "no events" (`latestSequence`), incremented by one per committed
//! event. Events are sent *before* the mutation (projectors run inside the
//! commit), which is what makes the sync/replay design in
//! reference/packages/opencode/src/sync/README.md work.
//!
//! TODO(integration): the reference backs this with SQLite (`EventTable` /
//! `EventSequenceTable` in `core/event/sql.ts`, see `sync::sql`); this port uses
//! an in-memory store with identical commit semantics so oc-database can back it
//! later without changing callers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use futures::stream::{self, BoxStream};
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::broadcast;

use super::event::{versioned_type, Definition, DurableEnvelope, EventID, LocationRef, Payload};

/// Error type mirroring `EventV2.InvalidDurableEventError` in
/// reference/packages/core/src/event.ts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InvalidDurableEvent {
    pub r#type: String,
    pub message: String,
}

pub type StoreError = InvalidDurableEvent;

impl InvalidDurableEvent {
    fn die(r#type: &str, message: impl Into<String>) -> Self {
        Self {
            r#type: r#type.to_string(),
            message: message.into(),
        }
    }
}

/// Registered durable event definitions by their versioned storage type, i.e.
/// `Event.durable([...])` in reference/packages/schema/src/durable-event-manifest.ts.
static DURABLE: OnceLock<Mutex<HashMap<String, Definition>>> = OnceLock::new();

fn durable_map() -> &'static Mutex<HashMap<String, Definition>> {
    DURABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Look up a durable definition by versioned type (e.g. `"session.next.moved.1"`).
/// Mirrors `Durable.get(...)`.
pub fn durable_get(r#type: &str) -> Option<Definition> {
    durable_map()
        .lock()
        .expect("durable manifest poisoned")
        .get(r#type)
        .cloned()
}

/// Register a durable definition, keyed by `versionedType(type, version)`.
/// Mirrors the `durable()` combinator populating the `Durable` map.
pub fn register_durable(def: Definition) {
    if let Some(durable) = &def.durable {
        durable_map()
            .lock()
            .expect("durable manifest poisoned")
            .insert(versioned_type(def.r#type, durable.version), def);
    }
}

/// Register the durable session definitions that flow through sync. Mirrors the
/// definitions in reference/packages/schema/src/durable-event-manifest.ts
/// (SessionV1 durable events + `SessionEvent.DurableDefinitions`).
pub fn register_session_durable_definitions() {
    let definitions = session_durable_definitions();
    for def in definitions {
        register_durable(def);
    }
}

/// The session durable definitions (type + version + aggregate). Both the v1
/// session events and the `session.next.*` events use aggregate `sessionID`.
pub fn session_durable_definitions() -> Vec<Definition> {
    let mut defs = Vec::new();
    let session_v1 = [
        "session.created",
        "session.updated",
        "session.deleted",
        "message.updated",
        "message.removed",
        "message.part.updated",
        "message.part.removed",
    ];
    for t in session_v1 {
        defs.push(Definition::durable(t, "sessionID", 1));
    }
    let session_next_v1 = [
        "session.next.agent.switched",
        "session.next.model.switched",
        "session.next.moved",
        "session.next.prompted",
        "session.next.prompt.admitted",
        "session.next.context.updated",
        "session.next.synthetic",
        "session.next.shell.started",
        "session.next.shell.ended",
        "session.next.step.started",
        "session.next.text.started",
        "session.next.text.ended",
        "session.next.tool.input.started",
        "session.next.tool.input.ended",
        "session.next.tool.called",
        "session.next.tool.progress",
        "session.next.tool.success",
        "session.next.tool.failed",
        "session.next.reasoning.started",
        "session.next.reasoning.ended",
        "session.next.retried",
        "session.next.compaction.started",
        "session.next.compaction.ended",
        "session.next.revert.staged",
        "session.next.revert.cleared",
        "session.next.revert.committed",
    ];
    for t in session_next_v1 {
        defs.push(Definition::durable(t, "sessionID", 1));
    }
    defs.push(Definition::durable(
        "session.next.step.ended",
        "sessionID",
        2,
    ));
    defs.push(Definition::durable(
        "session.next.step.failed",
        "sessionID",
        2,
    ));
    defs
}

/// A stored event row, mirroring `EventTable` in
/// reference/packages/core/src/event/sql.ts.
#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    pub id: EventID,
    pub aggregate_id: String,
    pub seq: i64,
    /// The versioned storage type, e.g. `session.next.moved.1`.
    pub r#type: String,
    pub data: Value,
}

/// A stored sequence row, mirroring `EventSequenceTable`.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceRow {
    pub aggregate_id: String,
    pub seq: i64,
    pub owner_id: Option<String>,
}

#[derive(Default)]
struct Db {
    events: Vec<EventRow>,
    sequences: HashMap<String, SequenceRow>,
}

pub type Projector = Arc<dyn Fn(&Payload) -> Result<(), StoreError> + Send + Sync>;
pub type Listener = Arc<dyn Fn(&Payload) + Send + Sync>;
type Commit = Box<dyn FnOnce(i64) -> Result<(), StoreError> + Send>;

/// Options for `Store::publish`, mirroring `EventV2.PublishOptions`.
pub struct PublishOptions {
    pub id: Option<EventID>,
    pub metadata: Option<Value>,
    pub location: Option<LocationRef>,
    /// Local operational projection committed atomically with the durable event.
    /// Not replayed or serialized.
    pub commit: Option<Commit>,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            id: None,
            metadata: None,
            location: None,
            commit: None,
        }
    }
}

/// Options for `Store::replay`, mirroring the replay options in `EventV2.replay`.
#[derive(Debug, Clone, Default)]
pub struct ReplayOptions {
    pub publish: bool,
    pub owner_id: Option<String>,
    pub strict_owner: bool,
}

struct StoreInner {
    db: Mutex<Db>,
    all: broadcast::Sender<Payload>,
    typed: Mutex<HashMap<String, broadcast::Sender<Payload>>>,
    wakes: Mutex<HashMap<String, broadcast::Sender<()>>>,
    projectors: Mutex<HashMap<String, Vec<Projector>>>,
    listeners: Mutex<Vec<Listener>>,
}

/// Cloneable handle to the in-memory event store.
#[derive(Clone)]
pub struct Store {
    inner: Arc<StoreInner>,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    /// Create a store with the session durable definitions registered.
    pub fn new() -> Self {
        register_session_durable_definitions();
        let (all, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(StoreInner {
                db: Mutex::new(Db::default()),
                all,
                typed: Mutex::new(HashMap::new()),
                wakes: Mutex::new(HashMap::new()),
                projectors: Mutex::new(HashMap::new()),
                listeners: Mutex::new(Vec::new()),
            }),
        }
    }

    fn db(&self) -> &Mutex<Db> {
        &self.inner.db
    }

    fn notify(&self, event: &Payload, isolate_listeners: bool) {
        let listeners = self.inner.listeners.lock().expect("listeners poisoned");
        if isolate_listeners {
            for listener in listeners.iter() {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(event)));
                if result.is_err() {
                    tracing::error!(event_id = %event.id, event_type = %event.r#type, "event listener panicked");
                }
            }
        } else {
            for listener in listeners.iter() {
                listener(event);
            }
        }
        drop(listeners);

        if let Some(typed) = self
            .inner
            .typed
            .lock()
            .expect("typed pubsub poisoned")
            .get(&event.r#type)
        {
            let _ = typed.send(event.clone());
        }
        let _ = self.inner.all.send(event.clone());
    }

    /// `publish` from reference/packages/core/src/event.ts (`publish`/`publishEvent`).
    pub fn publish(
        &self,
        definition: &Definition,
        data: Value,
        options: PublishOptions,
    ) -> Result<Payload, StoreError> {
        let id = options.id.clone().unwrap_or_else(EventID::create);
        let event = Payload {
            id,
            metadata: options.metadata.clone(),
            r#type: definition.r#type.to_string(),
            durable: None,
            location: options.location.clone(),
            data,
        };
        if definition.durable.is_none() && options.commit.is_some() {
            return Err(InvalidDurableEvent::die(
                &event.r#type,
                "Local commit hooks require a durable event",
            ));
        }
        let commit = options.commit;
        if let Some(committed) = self.commit_durable(definition, &event, None, commit)? {
            let mut event = event;
            event.durable = Some(committed.envelope());
            self.notify(&event, true);
            return Ok(event);
        }
        self.notify(&event, false);
        Ok(event)
    }

    /// `replay` from reference/packages/core/src/event.ts.
    pub fn replay(
        &self,
        event: &super::event::SerializedEvent,
        options: &ReplayOptions,
    ) -> Result<(), StoreError> {
        let definition = durable_get(&event.r#type).ok_or_else(|| {
            InvalidDurableEvent::die(
                &event.r#type,
                format!("Unknown durable event type {}", event.r#type),
            )
        })?;
        let payload = Payload {
            id: event.id.clone(),
            metadata: None,
            r#type: definition.r#type.to_string(),
            durable: None,
            location: None,
            data: event.data.clone(),
        };
        let input = ReplayInput {
            seq: event.seq,
            aggregate_id: event.aggregate_id.clone(),
            owner_id: options.owner_id.clone(),
            strict_owner: options.strict_owner,
        };
        let committed = self.commit_durable(&definition, &payload, Some(&input), None)?;
        if committed.is_some() && options.publish {
            let mut payload = payload;
            payload.durable = committed.as_ref().map(|c| c.envelope());
            self.notify(&payload, true);
        }
        Ok(())
    }

    /// `replayAll` from reference/packages/core/src/event.ts.
    pub fn replay_all(
        &self,
        events: &[super::event::SerializedEvent],
        options: &ReplayOptions,
    ) -> Result<Option<String>, StoreError> {
        let source = events.first().map(|e| e.aggregate_id.clone());
        let Some(source) = source else {
            return Ok(None);
        };
        if events.iter().any(|event| event.aggregate_id != source) {
            return Err(InvalidDurableEvent::die(
                events
                    .first()
                    .map(|e| e.r#type.as_str())
                    .unwrap_or("unknown"),
                "Replay events must belong to the same aggregate",
            ));
        }
        let start = events.first().map(|e| e.seq).unwrap_or(0);
        for (index, event) in events.iter().enumerate() {
            let expected = start + index as i64;
            if event.seq != expected {
                return Err(InvalidDurableEvent::die(
                    &event.r#type,
                    format!(
                        "Replay sequence mismatch at index {index}: expected {expected}, got {}",
                        event.seq
                    ),
                ));
            }
        }
        for event in events {
            self.replay(event, options)?;
        }
        Ok(Some(source))
    }

    /// `latestSequence` from reference/packages/core/src/event.ts.
    pub fn latest_sequence(&self, aggregate_id: &str) -> i64 {
        self.db()
            .lock()
            .expect("db poisoned")
            .sequences
            .get(aggregate_id)
            .map(|row| row.seq)
            .unwrap_or(-1)
    }

    /// `readAggregate` from reference/packages/core/src/event.ts.
    ///
    /// `limit` events with `seq > after` for the aggregate are returned ordered
    /// ascending, plus whether more remain. Only types present in `manifest`
    /// (storage types) are considered.
    pub fn read_aggregate(
        &self,
        aggregate_id: &str,
        after: Option<i64>,
        limit: usize,
        manifest: &[String],
    ) -> Result<(Vec<Payload>, bool), StoreError> {
        let after = after.unwrap_or(-1);
        let db = self.db().lock().expect("db poisoned");
        let mut rows: Vec<&EventRow> = db
            .events
            .iter()
            .filter(|e| {
                e.aggregate_id == aggregate_id && e.seq > after && manifest.contains(&e.r#type)
            })
            .collect();
        rows.sort_by_key(|e| e.seq);
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            events.push(self.decode_row(row)?);
        }
        Ok((events, has_more))
    }

    /// Read all events for an aggregate after a sequence, ascending. Mirrors the
    /// `readAfter` helper (plus `decodeSerializedEvent`) in reference/packages/core/src/event.ts.
    pub fn read_after(&self, aggregate_id: &str, after: i64) -> Result<Vec<Payload>, StoreError> {
        let db = self.db().lock().expect("db poisoned");
        let mut rows: Vec<&EventRow> = db
            .events
            .iter()
            .filter(|e| e.aggregate_id == aggregate_id && e.seq > after)
            .collect();
        rows.sort_by_key(|e| e.seq);
        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            events.push(self.decode_row(row)?);
        }
        Ok(events)
    }

    /// Decode a stored row into a `Payload`, resolving the canonical type and
    /// durable version through the registry. Mirrors `decodeSerializedEvent` in
    /// reference/packages/core/src/event.ts (which dies on unknown types).
    fn decode_row(&self, row: &EventRow) -> Result<Payload, StoreError> {
        let definition = durable_get(&row.r#type).ok_or_else(|| {
            InvalidDurableEvent::die(
                &row.r#type,
                format!("Unknown durable event type {}", row.r#type),
            )
        })?;
        let durable = definition.durable.as_ref().ok_or_else(|| {
            InvalidDurableEvent::die(
                &row.r#type,
                format!("Unknown durable event type {}", row.r#type),
            )
        })?;
        Ok(Payload {
            id: row.id.clone(),
            metadata: None,
            r#type: definition.r#type.to_string(),
            durable: Some(DurableEnvelope {
                aggregate_id: row.aggregate_id.clone(),
                seq: row.seq,
                version: durable.version,
            }),
            location: None,
            data: row.data.clone(),
        })
    }

    /// `remove` from reference/packages/core/src/event.ts.
    pub fn remove(&self, aggregate_id: &str) {
        let mut db = self.db().lock().expect("db poisoned");
        db.sequences.remove(aggregate_id);
        db.events.retain(|e| e.aggregate_id != aggregate_id);
    }

    /// `claim` from reference/packages/core/src/event.ts.
    pub fn claim(&self, aggregate_id: &str, owner_id: &str) {
        let mut db = self.db().lock().expect("db poisoned");
        if let Some(row) = db.sequences.get_mut(aggregate_id) {
            row.owner_id = Some(owner_id.to_string());
        }
    }

    /// Register a projector for a definition. Mirrors `EventV2.project`.
    pub fn project(&self, definition: &Definition, projector: Projector) {
        self.inner
            .projectors
            .lock()
            .expect("projectors poisoned")
            .entry(definition.r#type.to_string())
            .or_default()
            .push(projector);
    }

    /// Register a listener for all events. Mirrors `EventV2.listen`.
    pub fn listen(&self, listener: Listener) {
        self.inner
            .listeners
            .lock()
            .expect("listeners poisoned")
            .push(listener);
    }

    /// Subscribe to live events of a specific (unversioned) type.
    /// Mirrors `EventV2.subscribe`.
    pub fn subscribe(&self, definition: &Definition) -> broadcast::Receiver<Payload> {
        let mut typed = self.inner.typed.lock().expect("typed pubsub poisoned");
        let sender = typed
            .entry(definition.r#type.to_string())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(256);
                sender
            })
            .clone();
        sender.subscribe()
    }

    /// Subscribe to all live events. Mirrors `EventV2.all`.
    pub fn all(&self) -> broadcast::Receiver<Payload> {
        self.inner.all.subscribe()
    }

    /// Live + historical stream of durable events for an aggregate, starting
    /// strictly after `after`. Mirrors `EventV2.durable`.
    pub fn durable(&self, aggregate_id: &str, after: Option<i64>) -> BoxStream<'static, Payload> {
        let store = self.clone();
        let aggregate_id = aggregate_id.to_string();
        let mut sequence = after.unwrap_or(-1);

        let historical = store
            .read_after(&aggregate_id, sequence)
            .unwrap_or_else(|error| {
                tracing::error!(%error, "durable stream failed to read history");
                Vec::new()
            });
        if let Some(last) = historical.last() {
            if let Some(durable) = &last.durable {
                sequence = durable.seq;
            }
        }

        let wake_rx = store.subscribe_wakes(&aggregate_id);
        let live = stream::unfold(
            (store.clone(), aggregate_id.clone(), wake_rx, sequence),
            |(store, aggregate_id, mut wake_rx, sequence)| async move {
                match wake_rx.recv().await {
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        let events = store.read_after(&aggregate_id, sequence).unwrap_or_else(
                            |error| {
                                tracing::error!(%error, "durable stream failed to read after wake");
                                Vec::new()
                            },
                        );
                        let last = events
                            .last()
                            .and_then(|e| e.durable.as_ref())
                            .map(|d| d.seq)
                            .unwrap_or(sequence);
                        Some((events, (store, aggregate_id, wake_rx, last)))
                    }
                    Err(broadcast::error::RecvError::Closed) => None,
                }
            },
        )
        .map(stream::iter)
        .flatten();

        Box::pin(stream::iter(historical).chain(live))
    }

    /// Snapshot the stored rows for a session aggregate (used by session warp to
    /// ship the full event history to the target). Mirrors the `EventTable` query
    /// in `Workspace.sessionWarp` (reference/packages/opencode/src/control-plane/workspace.ts).
    pub fn history(&self, aggregate_id: &str) -> Vec<EventRow> {
        let db = self.db().lock().expect("db poisoned");
        let mut rows: Vec<EventRow> = db
            .events
            .iter()
            .filter(|e| e.aggregate_id == aggregate_id)
            .cloned()
            .collect();
        rows.sort_by_key(|e| e.seq);
        rows
    }

    /// Sequence cursors for a set of aggregate ids. Mirrors the
    /// `EventSequenceTable` select in `Workspace.syncHistory` /
    /// `synced` (reference/packages/opencode/src/control-plane/workspace.ts).
    pub fn sequences_for(&self, aggregate_ids: &[String]) -> HashMap<String, i64> {
        let db = self.db().lock().expect("db poisoned");
        aggregate_ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    db.sequences.get(id).map(|row| row.seq).unwrap_or(-1),
                )
            })
            .collect()
    }

    fn subscribe_wakes(&self, aggregate_id: &str) -> broadcast::Receiver<()> {
        let mut wakes = self.inner.wakes.lock().expect("wakes poisoned");
        let sender = wakes
            .entry(aggregate_id.to_string())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(16);
                sender
            })
            .clone();
        sender.subscribe()
    }

    fn wake_aggregate(&self, aggregate_id: &str) {
        let mut wakes = self.inner.wakes.lock().expect("wakes poisoned");
        if let Some(sender) = wakes.get(aggregate_id) {
            let _ = sender.send(());
            if sender.receiver_count() <= 1 {
                wakes.remove(aggregate_id);
            }
        }
    }
}

struct Committed {
    aggregate_id: String,
    seq: i64,
    version: i64,
}

impl Committed {
    fn envelope(&self) -> DurableEnvelope {
        DurableEnvelope {
            aggregate_id: self.aggregate_id.clone(),
            seq: self.seq,
            version: self.version,
        }
    }
}

#[derive(Debug, Clone)]
struct ReplayInput {
    seq: i64,
    aggregate_id: String,
    owner_id: Option<String>,
    strict_owner: bool,
}

impl Store {
    /// `commitDurableEvent` from reference/packages/core/src/event.ts, including
    /// the idempotent-replay, divergence, ownership, and sequence checks.
    fn commit_durable(
        &self,
        definition: &Definition,
        event: &Payload,
        input: Option<&ReplayInput>,
        commit: Option<Commit>,
    ) -> Result<Option<Committed>, StoreError> {
        let Some(durable) = &definition.durable else {
            return Ok(None);
        };

        let aggregate_field = event
            .data
            .get(&durable.aggregate)
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some(aggregate_id) = aggregate_field else {
            return Err(InvalidDurableEvent::die(
                &event.r#type,
                format!("Expected string aggregate field {}", durable.aggregate),
            ));
        };
        if let Some(input) = input {
            if input.aggregate_id != aggregate_id {
                return Err(InvalidDurableEvent::die(
                    &event.r#type,
                    format!(
                        "Aggregate mismatch: expected {}, got {}",
                        input.aggregate_id, aggregate_id
                    ),
                ));
            }
        }

        let projectors = self
            .inner
            .projectors
            .lock()
            .expect("projectors poisoned")
            .get(&event.r#type)
            .cloned()
            .unwrap_or_default();

        let mut db = self.db().lock().expect("db poisoned");

        let row = db.sequences.get(&aggregate_id).cloned();
        let latest = row.as_ref().map(|r| r.seq).unwrap_or(-1);
        let encoded = event.data.clone();
        let storage_type = definition.storage_type();
        let version = durable.version;

        if let Some(input) = input {
            let stored_owner = row.as_ref().and_then(|r| r.owner_id.clone());
            if input.strict_owner && stored_owner.is_some() && stored_owner != input.owner_id {
                let got = input.owner_id.clone().unwrap_or_else(|| "none".to_string());
                let expected = stored_owner.unwrap_or_default();
                return Err(InvalidDurableEvent::die(
                    &event.r#type,
                    format!(
                        "Replay owner mismatch for aggregate {aggregate_id}: expected {expected}, got {got}"
                    ),
                ));
            }
        }
        if let Some(input) = input {
            if input.seq <= latest {
                let stored = db
                    .events
                    .iter()
                    .find(|e| e.aggregate_id == aggregate_id && e.seq == input.seq);
                let idempotent = stored.is_some_and(|e| {
                    e.id == event.id && e.r#type == storage_type && e.data == encoded
                });
                if idempotent {
                    if input.owner_id.is_some()
                        && row.as_ref().and_then(|r| r.owner_id.clone()).is_none()
                    {
                        db.sequences
                            .get_mut(&aggregate_id)
                            .map(|r| r.owner_id = input.owner_id.clone());
                    }
                    return Ok(None);
                }
                return Err(InvalidDurableEvent::die(
                    &event.r#type,
                    format!(
                        "Replay diverged at aggregate {aggregate_id} sequence {}",
                        input.seq
                    ),
                ));
            }
            if row
                .as_ref()
                .and_then(|r| r.owner_id.as_ref())
                .is_some_and(|owner| Some(owner) != input.owner_id.as_ref())
            {
                return Ok(None);
            }
        }

        let seq = input.map(|i| i.seq).unwrap_or(latest + 1);
        if let Some(input) = input {
            if seq != latest + 1 {
                return Err(InvalidDurableEvent::die(
                    &event.r#type,
                    format!(
                        "Sequence mismatch for aggregate {aggregate_id}: expected {}, got {}",
                        latest + 1,
                        input.seq
                    ),
                ));
            }
        }
        if let Some(stored) = db.events.iter().find(|e| e.id == event.id) {
            return Err(InvalidDurableEvent::die(
                &event.r#type,
                format!(
                    "Event {} already exists at aggregate {} sequence {}",
                    event.id, stored.aggregate_id, stored.seq
                ),
            ));
        }

        let committed = Committed {
            aggregate_id: aggregate_id.clone(),
            seq,
            version,
        };
        let mut committed_payload = event.clone();
        committed_payload.durable = Some(committed.envelope());
        for projector in &projectors {
            projector(&committed_payload)?;
        }
        if let Some(commit) = commit {
            commit(seq)?;
        }

        let owner_id = if row.as_ref().and_then(|r| r.owner_id.as_ref()).is_some() {
            row.as_ref().and_then(|r| r.owner_id.clone())
        } else {
            input.and_then(|i| i.owner_id.clone())
        };
        db.sequences.insert(
            aggregate_id.clone(),
            SequenceRow {
                aggregate_id: aggregate_id.clone(),
                seq,
                owner_id,
            },
        );
        db.events.push(EventRow {
            id: event.id.clone(),
            aggregate_id: aggregate_id.clone(),
            seq,
            r#type: storage_type,
            data: encoded,
        });
        drop(db);

        self.wake_aggregate(&aggregate_id);
        Ok(Some(committed))
    }
}

#[cfg(test)]
mod tests {
    use super::super::event::SerializedEvent;
    use super::*;
    use std::sync::Arc;

    fn moved(data: Value) -> SerializedEvent {
        SerializedEvent {
            id: EventID::create(),
            r#type: "session.next.moved.1".into(),
            seq: 0,
            aggregate_id: "ses_1".into(),
            data,
        }
    }

    #[test]
    fn publish_assigns_incrementing_cursors() {
        let store = Store::new();
        let def = Definition::durable("session.next.moved", "sessionID", 1);

        let first = store
            .publish(
                &def,
                serde_json::json!({ "sessionID": "ses_1", "location": {} }),
                PublishOptions::default(),
            )
            .unwrap();
        let second = store
            .publish(
                &def,
                serde_json::json!({ "sessionID": "ses_1", "location": {} }),
                PublishOptions::default(),
            )
            .unwrap();

        assert_eq!(store.latest_sequence("ses_1"), 1);
        assert_eq!(first.durable.unwrap().seq, 0);
        assert_eq!(second.durable.unwrap().seq, 1);
    }

    #[test]
    fn publish_requires_aggregate_field() {
        let store = Store::new();
        let def = Definition::durable("session.next.moved", "sessionID", 1);
        let err = store
            .publish(&def, serde_json::json!({}), PublishOptions::default())
            .unwrap_err();
        assert!(
            err.message
                .contains("Expected string aggregate field sessionID"),
            "{err}"
        );
    }

    #[test]
    fn sequences_are_per_aggregate() {
        let store = Store::new();
        let def = Definition::durable("session.next.moved", "sessionID", 1);
        store
            .publish(
                &def,
                serde_json::json!({ "sessionID": "ses_a" }),
                PublishOptions::default(),
            )
            .unwrap();
        store
            .publish(
                &def,
                serde_json::json!({ "sessionID": "ses_b" }),
                PublishOptions::default(),
            )
            .unwrap();
        assert_eq!(store.latest_sequence("ses_a"), 0);
        assert_eq!(store.latest_sequence("ses_b"), 0);
    }

    #[test]
    fn replay_commits_at_cursor() {
        let store = Store::new();
        let event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        store.replay(&event, &ReplayOptions::default()).unwrap();
        assert_eq!(store.latest_sequence("ses_1"), 0);
        assert_eq!(store.history("ses_1").len(), 1);
    }

    #[test]
    fn replay_is_idempotent() {
        let store = Store::new();
        let event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        store.replay(&event, &ReplayOptions::default()).unwrap();
        store.replay(&event, &ReplayOptions::default()).unwrap();
        assert_eq!(store.latest_sequence("ses_1"), 0);
        assert_eq!(store.history("ses_1").len(), 1);
    }

    #[test]
    fn replay_divergence_is_detected() {
        let store = Store::new();
        let first = moved(serde_json::json!({ "sessionID": "ses_1", "v": 1 }));
        let conflicting = moved(serde_json::json!({ "sessionID": "ses_1", "v": 2 }));
        store.replay(&first, &ReplayOptions::default()).unwrap();
        let err = store
            .replay(&conflicting, &ReplayOptions::default())
            .unwrap_err();
        assert!(
            err.message
                .contains("Replay diverged at aggregate ses_1 sequence 0"),
            "{err}"
        );
    }

    #[test]
    fn replay_sequence_mismatch_is_detected() {
        let store = Store::new();
        let mut event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        event.seq = 5;
        let err = store.replay(&event, &ReplayOptions::default()).unwrap_err();
        assert!(
            err.message
                .contains("Sequence mismatch for aggregate ses_1: expected 0, got 5"),
            "{err}"
        );
    }

    #[test]
    fn replay_rejects_unknown_durable_type() {
        let store = Store::new();
        let mut event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        event.r#type = "unknown.event.1".into();
        let err = store.replay(&event, &ReplayOptions::default()).unwrap_err();
        assert!(
            err.message
                .contains("Unknown durable event type unknown.event.1"),
            "{err}"
        );
    }

    #[test]
    fn replay_all_validates_same_aggregate_and_contiguous_cursors() {
        let store = Store::new();
        let a = moved(serde_json::json!({ "sessionID": "ses_1" }));
        let mut b = moved(serde_json::json!({ "sessionID": "ses_1" }));
        b.seq = 1;
        let source = store
            .replay_all(&[a.clone(), b.clone()], &ReplayOptions::default())
            .unwrap();
        assert_eq!(source.as_deref(), Some("ses_1"));

        let mut cross = moved(serde_json::json!({ "sessionID": "ses_2" }));
        cross.aggregate_id = "ses_2".into();
        let err = store
            .replay_all(&[a.clone(), cross], &ReplayOptions::default())
            .unwrap_err();
        assert!(
            err.message
                .contains("Replay events must belong to the same aggregate"),
            "{err}"
        );

        let mut gap = moved(serde_json::json!({ "sessionID": "ses_3" }));
        gap.seq = 2;
        let err = store
            .replay_all(&[a.clone(), gap], &ReplayOptions::default())
            .unwrap_err();
        assert!(
            err.message.contains("Replay sequence mismatch at index 1"),
            "{err}"
        );
    }

    #[test]
    fn owner_claim_steals_session() {
        let store = Store::new();
        let event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        store
            .replay(
                &event,
                &ReplayOptions {
                    publish: false,
                    owner_id: Some("wrk_a".into()),
                    strict_owner: false,
                },
            )
            .unwrap();
        store.claim("ses_1", "wrk_b");

        // A new event from a foreign owner (the pre-claim owner) is ignored.
        let mut foreign = moved(serde_json::json!({ "sessionID": "ses_1", "v": 2 }));
        foreign.seq = 1;
        store
            .replay(
                &foreign,
                &ReplayOptions {
                    publish: false,
                    owner_id: Some("wrk_a".into()),
                    strict_owner: false,
                },
            )
            .unwrap();
        assert_eq!(store.latest_sequence("ses_1"), 0);
        assert_eq!(store.history("ses_1").len(), 1);
    }

    #[test]
    fn strict_owner_rejects_mismatch() {
        let store = Store::new();
        let event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        store
            .replay(
                &event,
                &ReplayOptions {
                    publish: false,
                    owner_id: Some("wrk_a".into()),
                    strict_owner: false,
                },
            )
            .unwrap();
        let err = store
            .replay(
                &moved(serde_json::json!({ "sessionID": "ses_1", "v": 2 })),
                &ReplayOptions {
                    publish: false,
                    owner_id: Some("wrk_b".into()),
                    strict_owner: true,
                },
            )
            .unwrap_err();
        assert!(
            err.message
                .contains("Replay owner mismatch for aggregate ses_1"),
            "{err}"
        );
    }

    #[test]
    fn replay_publish_notifies_listeners() {
        let store = Store::new();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        store.listen(Arc::new(move |payload: &Payload| {
            seen_clone
                .lock()
                .unwrap()
                .push((payload.r#type.clone(), payload.durable.clone()));
        }));
        let event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        store
            .replay(
                &event,
                &ReplayOptions {
                    publish: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "session.next.moved");
        assert_eq!(seen[0].1.as_ref().unwrap().seq, 0);
    }

    #[test]
    fn read_aggregate_paginates_by_cursor() {
        let store = Store::new();
        let def = Definition::durable("session.next.moved", "sessionID", 1);
        for i in 0..5 {
            store
                .publish(
                    &def,
                    serde_json::json!({ "sessionID": "ses_1", "i": i }),
                    PublishOptions::default(),
                )
                .unwrap();
        }
        let manifest = vec![def.storage_type()];
        let (page, has_more) = store.read_aggregate("ses_1", None, 2, &manifest).unwrap();
        assert_eq!(page.len(), 2);
        assert!(has_more);
        assert_eq!(page[0].durable.as_ref().unwrap().seq, 0);
        assert_eq!(page[1].durable.as_ref().unwrap().seq, 1);

        let (page, has_more) = store
            .read_aggregate("ses_1", Some(1), 2, &manifest)
            .unwrap();
        assert_eq!(page.len(), 2);
        assert!(has_more);
        assert_eq!(page[0].durable.as_ref().unwrap().seq, 2);

        let (page, has_more) = store
            .read_aggregate("ses_1", Some(3), 2, &manifest)
            .unwrap();
        assert_eq!(page.len(), 1);
        assert!(!has_more);
    }

    #[test]
    fn publish_rejects_event_id_collision() {
        let store = Store::new();
        let def = Definition::durable("session.next.moved", "sessionID", 1);
        let id = EventID::create();
        store
            .publish(
                &def,
                serde_json::json!({ "sessionID": "ses_1" }),
                PublishOptions {
                    id: Some(id.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        let err = store
            .publish(
                &def,
                serde_json::json!({ "sessionID": "ses_1" }),
                PublishOptions {
                    id: Some(id.clone()),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(err.message.contains("already exists"), "{err}");
    }

    #[test]
    fn remove_deletes_aggregate() {
        let store = Store::new();
        let event = moved(serde_json::json!({ "sessionID": "ses_1" }));
        store.replay(&event, &ReplayOptions::default()).unwrap();
        store.remove("ses_1");
        assert_eq!(store.latest_sequence("ses_1"), -1);
        assert!(store.history("ses_1").is_empty());
    }
}
