//! Durable event storage abstraction.
//!
//! From reference/packages/core/src/event.ts — the database-backed durable
//! event tables are owned by oc-database; this trait is the seam.
//! TODO(integration): provide a SQLite-backed store in oc-database matching
//! `event/sql.ts` (`event_sequence`, `event` tables).

use std::future::Future;
use std::pin::Pin;

use serde_json::{Map, Value};

/// A row in the `event` table (`{ id, aggregate_id, seq, type, data }`).
/// From reference/packages/core/src/event.ts + event/sql.ts
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub id: String,
    pub aggregate_id: String,
    pub seq: i64,
    /// Stored type — `versionedType(type, version)` for durable events.
    pub r#type: String,
    pub data: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    pub message: String,
}

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        StoreError {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}

/// Read/write view handed to `DurableStore::transaction`.
pub trait DurableTx: Sync {
    /// `(seq, owner_id)` for an aggregate, or `None` if no sequence row.
    fn sequence(&self, aggregate_id: &str) -> Option<(i64, Option<String>)>;

    fn stored_event_by_id(&self, id: &str) -> Option<StoredEvent>;

    fn stored_event_at(&self, aggregate_id: &str, seq: i64) -> Option<StoredEvent>;

    /// Upsert the aggregate's sequence row. `owner` is applied only when
    /// `set_owner` is true (mirroring the reference's conditional update).
    fn upsert_sequence(
        &self,
        aggregate_id: &str,
        seq: i64,
        owner: Option<String>,
        set_owner: bool,
    ) -> Result<(), StoreError>;

    fn insert_event(&self, event: StoredEvent) -> Result<(), StoreError>;
}

/// Error type used by [`DurableStore::transaction`]. The store is
/// transport-agnostic; the caller boxes its own error (e.g. the bus's
/// `InvalidDurableEventError`).
pub type TransactionError = Box<dyn std::error::Error + Send + Sync>;

/// Result of a store transaction. The value is `Box<dyn Any>` so the trait
/// stays dyn-compatible; the caller downcasts to its concrete commit result.
pub type TxResult = Result<Box<dyn std::any::Any + Send>, TransactionError>;

/// A transactional closure: observes and mutates rows through `DurableTx`.
pub type TxClosure = Box<
    dyn for<'a> FnOnce(&'a dyn DurableTx) -> Pin<Box<dyn Future<Output = TxResult> + Send + 'a>>
        + Send,
>;

/// A boxed future returned by store methods.
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Transactional durable event store.
pub trait DurableStore: Send + Sync {
    /// `latestSequence(db, aggregateID)`.
    fn latest_sequence<'a>(&'a self, aggregate_id: &'a str) -> StoreFuture<'a, i64>;

    /// Rows for `aggregate_id` with `seq > after`, ascending by seq.
    fn read_after<'a>(
        &'a self,
        aggregate_id: &'a str,
        after: i64,
    ) -> StoreFuture<'a, Vec<StoredEvent>>;

    /// Run `f` atomically.
    fn transaction<'a>(&'a self, f: TxClosure) -> StoreFuture<'a, TxResult>;

    /// `remove(aggregateID)` — deletes the sequence row and all events.
    fn remove_aggregate<'a>(
        &'a self,
        aggregate_id: &'a str,
    ) -> StoreFuture<'a, Result<(), StoreError>>;

    /// `claim(aggregateID, ownerID)` — sets the sequence owner.
    fn claim<'a>(
        &'a self,
        aggregate_id: &'a str,
        owner_id: &'a str,
    ) -> StoreFuture<'a, Result<(), StoreError>>;
}

/// Process-local in-memory durable store. Not durable across restarts.
///
/// Transactions are serialized by an async (tokio) lock whose guard is `Send`;
/// the underlying data locks are only ever held briefly inside a single
/// `DurableTx` call, never across an await.
#[derive(Debug, Default)]
pub struct InMemoryDurableStore {
    tx: tokio::sync::Mutex<()>,
    sequences: std::sync::Mutex<std::collections::HashMap<String, (i64, Option<String>)>>,
    events: std::sync::Mutex<Vec<StoredEvent>>,
}

impl InMemoryDurableStore {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Read/write view for a transaction: holds references to the data locks and
/// touches them per call. The transaction serialization lock guarantees the
/// view is exclusive.
struct TxView<'a> {
    sequences: &'a std::sync::Mutex<std::collections::HashMap<String, (i64, Option<String>)>>,
    events: &'a std::sync::Mutex<Vec<StoredEvent>>,
}

impl DurableTx for TxView<'_> {
    fn sequence(&self, aggregate_id: &str) -> Option<(i64, Option<String>)> {
        self.sequences.lock().unwrap().get(aggregate_id).cloned()
    }

    fn stored_event_by_id(&self, id: &str) -> Option<StoredEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .find(|event| event.id == id)
            .cloned()
    }

    fn stored_event_at(&self, aggregate_id: &str, seq: i64) -> Option<StoredEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .find(|event| event.aggregate_id == aggregate_id && event.seq == seq)
            .cloned()
    }

    fn upsert_sequence(
        &self,
        aggregate_id: &str,
        seq: i64,
        owner: Option<String>,
        set_owner: bool,
    ) -> Result<(), StoreError> {
        let mut guard = self.sequences.lock().unwrap();
        let entry = guard.entry(aggregate_id.to_string()).or_insert((seq, None));
        entry.0 = seq;
        if set_owner {
            entry.1 = owner;
        }
        Ok(())
    }

    fn insert_event(&self, event: StoredEvent) -> Result<(), StoreError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

impl DurableStore for InMemoryDurableStore {
    fn latest_sequence<'a>(&'a self, aggregate_id: &'a str) -> StoreFuture<'a, i64> {
        Box::pin(async move {
            self.sequences
                .lock()
                .unwrap()
                .get(aggregate_id)
                .map(|(seq, _)| *seq)
                .unwrap_or(-1)
        })
    }

    fn read_after<'a>(
        &'a self,
        aggregate_id: &'a str,
        after: i64,
    ) -> StoreFuture<'a, Vec<StoredEvent>> {
        Box::pin(async move {
            let mut rows: Vec<StoredEvent> = self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.aggregate_id == aggregate_id && event.seq > after)
                .cloned()
                .collect();
            rows.sort_by_key(|event| event.seq);
            rows
        })
    }

    fn transaction<'a>(&'a self, f: TxClosure) -> StoreFuture<'a, TxResult> {
        Box::pin(async move {
            let _tx = self.tx.lock().await;
            let view = TxView {
                sequences: &self.sequences,
                events: &self.events,
            };
            f(&view).await
        })
    }

    fn remove_aggregate<'a>(
        &'a self,
        aggregate_id: &'a str,
    ) -> StoreFuture<'a, Result<(), StoreError>> {
        Box::pin(async move {
            self.sequences.lock().unwrap().remove(aggregate_id);
            self.events
                .lock()
                .unwrap()
                .retain(|event| event.aggregate_id != aggregate_id);
            Ok(())
        })
    }

    fn claim<'a>(
        &'a self,
        aggregate_id: &'a str,
        owner_id: &'a str,
    ) -> StoreFuture<'a, Result<(), StoreError>> {
        Box::pin(async move {
            if let Some((_, owner)) = self.sequences.lock().unwrap().get_mut(aggregate_id) {
                *owner = Some(owner_id.to_string());
            }
            Ok(())
        })
    }
}
