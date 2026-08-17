//! Async event bus.
//!
//! From reference/packages/core/src/event.ts — `EventV2.Service`. Approximates
//! the Effect PubSub/Stream plumbing with tokio channels. Durable commits go
//! through the [`DurableStore`] trait (in-memory by default; SQLite-backed via
//! oc-database during integration).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::durable::{DurableStore, StoreError, StoredEvent};
use crate::event::{
    encode_data, versioned_type, Definition, DurableInfo, DurableRegistry,
    InvalidDurableEventError, Payload, SerializedEvent,
};
use crate::ids::EventId;
use crate::location::LocationRef;
use crate::state::BoxFuture;

pub type Listener = Arc<dyn Fn(&Payload) -> BoxFuture<'static, ()> + Send + Sync>;

#[derive(Default)]
pub struct PublishOptions {
    pub id: Option<EventId>,
    pub metadata: Option<Map<String, Value>>,
    pub location: Option<LocationRef>,
    /// Local operational projection committed atomically with a new durable
    /// event. Not replayed or serialized.
    pub commit: Option<Arc<dyn Fn(i64) -> BoxFuture<'static, ()> + Send + Sync>>,
}

#[derive(Debug, Clone)]
pub struct CommitInput {
    pub seq: i64,
    pub aggregate_id: String,
    pub owner_id: Option<String>,
    pub strict_owner: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct ReplayOptions {
    pub publish: bool,
    pub owner_id: Option<String>,
    pub strict_owner: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum BusError {
    #[error(transparent)]
    InvalidDurable(#[from] InvalidDurableEventError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

struct BusInner {
    all_subs: Mutex<Vec<mpsc::UnboundedSender<Payload>>>,
    typed_subs: Mutex<HashMap<String, Vec<mpsc::UnboundedSender<Payload>>>>,
    durable_wakes: Mutex<HashMap<String, Vec<watch::Sender<()>>>>,
    listeners: RwLock<Vec<Listener>>,
    projectors: RwLock<HashMap<String, Vec<Listener>>>,
    store: Arc<dyn DurableStore>,
    registry: Arc<DurableRegistry>,
}

impl Default for BusInner {
    fn default() -> Self {
        BusInner {
            all_subs: Mutex::new(Vec::new()),
            typed_subs: Mutex::new(HashMap::new()),
            durable_wakes: Mutex::new(HashMap::new()),
            listeners: RwLock::new(Vec::new()),
            projectors: RwLock::new(HashMap::new()),
            store: Arc::new(crate::durable::InMemoryDurableStore::new()),
            registry: Arc::new(DurableRegistry::default()),
        }
    }
}

/// The event bus service (`EventV2.Service`).
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<BusInner>,
}

impl EventBus {
    pub fn new(store: Arc<dyn DurableStore>, registry: Arc<DurableRegistry>) -> Self {
        EventBus {
            inner: Arc::new(BusInner {
                store,
                registry,
                ..BusInner::default()
            }),
        }
    }

    /// A bus with an in-memory durable store and an empty registry.
    pub fn in_memory() -> Self {
        EventBus {
            inner: Arc::new(BusInner::default()),
        }
    }

    pub fn registry(&self) -> &Arc<DurableRegistry> {
        &self.inner.registry
    }

    /// `latestSequence(db, aggregateID)`.
    pub async fn latest_sequence(&self, aggregate_id: &str) -> i64 {
        self.inner.store.latest_sequence(aggregate_id).await
    }

    /// `publish(definition, data, options)`.
    pub async fn publish(
        &self,
        definition: &Definition,
        data: &Map<String, Value>,
        options: &PublishOptions,
    ) -> Result<Payload, BusError> {
        if definition.durable.is_none() && options.commit.is_some() {
            return Err(BusError::InvalidDurable(InvalidDurableEventError::new(
                definition.r#type.clone(),
                "Local commit hooks require a durable event",
            )));
        }
        let encoded = encode_data(definition, data).map_err(|message| {
            BusError::InvalidDurable(InvalidDurableEventError::new(
                definition.r#type.clone(),
                message,
            ))
        })?;
        let mut event = Payload {
            id: options.id.clone().unwrap_or_else(EventId::create),
            metadata: options.metadata.clone(),
            r#type: definition.r#type.clone(),
            durable: None,
            location: options.location.clone(),
            data: encoded,
        };
        if let Some(durable) = &definition.durable {
            let committed = self
                .commit_durable(
                    definition.clone(),
                    durable.clone(),
                    event.clone(),
                    None,
                    options.commit.clone(),
                )
                .await?;
            if let Some((aggregate_id, seq)) = committed {
                event.durable = Some(DurableInfo {
                    aggregateID: aggregate_id,
                    seq,
                    version: durable.version,
                });
                self.notify(&event, true).await;
                return Ok(event);
            }
        }
        self.notify(&event, false).await;
        Ok(event)
    }

    /// Subscribe to events of one type. Mirrors `subscribe(definition)`.
    pub async fn subscribe(&self, definition_type: &str) -> mpsc::UnboundedReceiver<Payload> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.inner
            .typed_subs
            .lock()
            .await
            .entry(definition_type.to_string())
            .or_default()
            .push(tx);
        rx
    }

    /// Subscribe to every event. Mirrors `all()`.
    pub async fn subscribe_all(&self) -> mpsc::UnboundedReceiver<Payload> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.inner.all_subs.lock().await.push(tx);
        rx
    }

    /// Register a global listener; returns an unsubscribe handle.
    /// Mirrors `listen(listener)`.
    pub async fn listen(&self, listener: Listener) -> Unsubscribe {
        self.inner.listeners.write().await.push(listener.clone());
        Unsubscribe { listener }
    }

    /// Register a projector invoked before a durable event is committed.
    /// Mirrors `project(definition, projector)`.
    pub async fn project(&self, definition_type: &str, projector: Listener) {
        self.inner
            .projectors
            .write()
            .await
            .entry(definition_type.to_string())
            .or_default()
            .push(projector);
    }

    /// Stream of durable events for one aggregate. Mirrors `durable(input)`.
    /// Historical events are read first, then live events as they commit.
    pub fn durable(&self, aggregate_id: String, after: i64) -> mpsc::UnboundedReceiver<Payload> {
        let (tx, rx) = mpsc::unbounded_channel();
        let me = self.clone();
        tokio::spawn(async move {
            let mut sequence = after;
            if !forward_durable(&me, &aggregate_id, &tx, &mut sequence).await {
                return;
            }
            let mut wake = match me.subscribe_durable(&aggregate_id).await {
                Some(receiver) => receiver,
                None => return,
            };
            while wake.changed().await.is_ok() {
                if !forward_durable(&me, &aggregate_id, &tx, &mut sequence).await {
                    return;
                }
            }
        });
        rx
    }

    async fn subscribe_durable(&self, aggregate_id: &str) -> Option<watch::Receiver<()>> {
        let (tx, rx) = watch::channel(());
        self.inner
            .durable_wakes
            .lock()
            .await
            .entry(aggregate_id.to_string())
            .or_default()
            .push(tx);
        Some(rx)
    }

    async fn wake_durable(&self, aggregate_id: &str) {
        let wakes = self
            .inner
            .durable_wakes
            .lock()
            .await
            .get(aggregate_id)
            .cloned()
            .unwrap_or_default();
        for wake in wakes {
            let _ = wake.send(());
        }
    }

    /// `readAggregate(db, input)`.
    pub async fn read_aggregate(
        &self,
        aggregate_id: &str,
        after: i64,
        limit: usize,
        manifest_types: &[String],
    ) -> Result<(Vec<Payload>, bool), BusError> {
        let rows = self.inner.store.read_after(aggregate_id, after).await;
        let rows: Vec<StoredEvent> = rows
            .into_iter()
            .filter(|row| manifest_types.contains(&row.r#type))
            .collect();
        let has_more = rows.len() > limit;
        let page = rows.into_iter().take(limit).collect::<Vec<_>>();
        let mut events = Vec::with_capacity(page.len());
        for row in page {
            let definition = self
                .inner
                .registry
                .get_durable(&row.r#type)
                .ok_or_else(|| {
                    BusError::InvalidDurable(InvalidDurableEventError::new(
                        row.r#type.clone(),
                        format!("Unknown durable event type {}", row.r#type),
                    ))
                })?;
            let durable = definition.durable.as_ref().ok_or_else(|| {
                BusError::InvalidDurable(InvalidDurableEventError::new(
                    row.r#type.clone(),
                    format!("Unknown durable event type {}", row.r#type),
                ))
            })?;
            events.push(Payload {
                id: EventId(row.id.clone()),
                r#type: definition.r#type.clone(),
                durable: Some(DurableInfo {
                    aggregateID: row.aggregate_id.clone(),
                    seq: row.seq,
                    version: durable.version,
                }),
                metadata: None,
                location: None,
                data: row.data,
            });
        }
        Ok((events, has_more))
    }

    /// `replay(event, options)`.
    pub async fn replay(
        &self,
        event: SerializedEvent,
        options: &ReplayOptions,
    ) -> Result<(), BusError> {
        let definition = self
            .inner
            .registry
            .get_durable(&event.r#type)
            .ok_or_else(|| {
                BusError::InvalidDurable(InvalidDurableEventError::new(
                    event.r#type.clone(),
                    format!("Unknown durable event type {}", event.r#type),
                ))
            })?;
        let durable = definition.durable.as_ref().ok_or_else(|| {
            BusError::InvalidDurable(InvalidDurableEventError::new(
                event.r#type.clone(),
                format!("Unknown durable event type {}", event.r#type),
            ))
        })?;
        let payload = Payload {
            id: event.id.clone(),
            r#type: definition.r#type.clone(),
            data: event.data.clone(),
            metadata: None,
            durable: None,
            location: None,
        };
        let committed = self
            .commit_durable(
                definition.clone(),
                durable.clone(),
                payload.clone(),
                Some(CommitInput {
                    seq: event.seq,
                    aggregate_id: event.aggregateID.clone(),
                    owner_id: options.owner_id.clone(),
                    strict_owner: if options.strict_owner {
                        Some(true)
                    } else {
                        None
                    },
                }),
                None,
            )
            .await?;
        if committed.is_some() && options.publish {
            self.notify(&payload, true).await;
        }
        Ok(())
    }

    /// `replayAll(events, options)`.
    pub async fn replay_all(
        &self,
        events: Vec<SerializedEvent>,
        options: &ReplayOptions,
    ) -> Result<Option<String>, BusError> {
        let source = events.first().map(|event| event.aggregateID.clone());
        let Some(source) = source else {
            return Ok(None);
        };
        if events.iter().any(|event| event.aggregateID != source) {
            return Err(BusError::InvalidDurable(InvalidDurableEventError::new(
                events
                    .first()
                    .map(|e| e.r#type.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                "Replay events must belong to the same aggregate",
            )));
        }
        let start = events.first().map(|event| event.seq).unwrap_or(0);
        for (index, event) in events.iter().enumerate() {
            let expected = start + index as i64;
            if event.seq != expected {
                return Err(BusError::InvalidDurable(InvalidDurableEventError::new(
                    event.r#type.clone(),
                    format!(
                        "Replay sequence mismatch at index {index}: expected {expected}, got {}",
                        event.seq
                    ),
                )));
            }
        }
        for event in events {
            self.replay(event, options).await?;
        }
        Ok(Some(source))
    }

    /// `remove(aggregateID)`.
    pub async fn remove(&self, aggregate_id: &str) -> Result<(), BusError> {
        self.inner.store.remove_aggregate(aggregate_id).await?;
        Ok(())
    }

    /// `claim(aggregateID, ownerID)`.
    pub async fn claim(&self, aggregate_id: &str, owner_id: &str) -> Result<(), BusError> {
        self.inner.store.claim(aggregate_id, owner_id).await?;
        Ok(())
    }

    /// `allBounded(events, capacity)` — bounded all-events stream. Overflowing
    /// events are dropped. TODO(integration): surface `SubscriberOverflowError`
    /// like the reference's bounded queue.
    pub fn all_bounded(&self, capacity: usize) -> mpsc::Receiver<Payload> {
        let (tx, rx) = mpsc::channel(capacity);
        let me = self.clone();
        tokio::spawn(async move {
            let mut stream = me.subscribe_all().await;
            while let Some(event) = stream.recv().await {
                let _ = tx.send(event).await;
            }
        });
        rx
    }

    fn decode_stored(&self, stored: &StoredEvent) -> Result<Payload, BusError> {
        let definition = self
            .inner
            .registry
            .get_durable(&stored.r#type)
            .ok_or_else(|| {
                BusError::InvalidDurable(InvalidDurableEventError::new(
                    stored.r#type.clone(),
                    format!("Unknown durable event type {}", stored.r#type),
                ))
            })?;
        let durable = definition.durable.as_ref().ok_or_else(|| {
            BusError::InvalidDurable(InvalidDurableEventError::new(
                stored.r#type.clone(),
                format!("Unknown durable event type {}", stored.r#type),
            ))
        })?;
        Ok(Payload {
            id: EventId(stored.id.clone()),
            r#type: definition.r#type.clone(),
            durable: Some(DurableInfo {
                aggregateID: stored.aggregate_id.clone(),
                seq: stored.seq,
                version: durable.version,
            }),
            metadata: None,
            location: None,
            data: stored.data.clone(),
        })
    }

    async fn notify(&self, event: &Payload, isolate_listeners: bool) {
        let listeners = self.inner.listeners.read().await.clone();
        if isolate_listeners {
            for listener in listeners {
                let event = event.clone();
                tokio::spawn(async move {
                    (listener)(&event).await;
                });
            }
        } else {
            for listener in &listeners {
                (listener)(event).await;
            }
        }
        if let Some(subscribers) = self
            .inner
            .typed_subs
            .lock()
            .await
            .get(&event.r#type)
            .cloned()
        {
            for subscriber in subscribers {
                let _ = subscriber.send(event.clone());
            }
        }
        let all = self.inner.all_subs.lock().await.clone();
        for subscriber in all {
            let _ = subscriber.send(event.clone());
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_durable(
        &self,
        definition: Definition,
        durable: crate::event::Durable,
        event: Payload,
        input: Option<CommitInput>,
        commit_hook: Option<Arc<dyn Fn(i64) -> BoxFuture<'static, ()> + Send + Sync>>,
    ) -> Result<Option<(String, i64)>, BusError> {
        let aggregate_field = event.data.get(&durable.aggregate).and_then(Value::as_str);
        let Some(aggregate) = aggregate_field else {
            return Err(BusError::InvalidDurable(InvalidDurableEventError::new(
                event.r#type.clone(),
                format!("Expected string aggregate field {}", durable.aggregate),
            )));
        };
        let aggregate = aggregate.to_string();
        if let Some(input) = &input {
            if input.aggregate_id != aggregate {
                return Err(BusError::InvalidDurable(InvalidDurableEventError::new(
                    event.r#type.clone(),
                    format!(
                        "Aggregate mismatch: expected {}, got {aggregate}",
                        input.aggregate_id
                    ),
                )));
            }
        }
        let projectors = self
            .inner
            .projectors
            .read()
            .await
            .get(&event.r#type)
            .cloned()
            .unwrap_or_default();
        let encoded = encode_data(&definition, &event.data).map_err(|message| {
            BusError::InvalidDurable(InvalidDurableEventError::new(event.r#type.clone(), message))
        })?;
        let aggregate_for_commit = aggregate.clone();

        let committed = self
            .inner
            .store
            .transaction(Box::new(move |tx: &dyn crate::durable::DurableTx| {
                Box::pin(async move {
                    let aggregate = &aggregate_for_commit;
                    let row = tx.sequence(aggregate);
                    let latest = row.as_ref().map(|(seq, _)| *seq).unwrap_or(-1);
                    let strict_owner = input.as_ref().and_then(|i| i.strict_owner).unwrap_or(false);
                    if strict_owner {
                        if let Some((_, Some(owner))) = &row {
                            if input.as_ref().and_then(|i| i.owner_id.as_ref()) != Some(owner) {
                                return Err(InvalidDurableEventError::new(
                                    event.r#type.clone(),
                                    format!(
                                        "Replay owner mismatch for aggregate {aggregate}: expected {owner}, got {}",
                                        input.as_ref().and_then(|i| i.owner_id.as_deref()).unwrap_or("none")
                                    ),
                                )
                                .into());
                            }
                        }
                    }
                    if let Some(input) = &input {
                        if input.seq <= latest {
                            let stored = tx.stored_event_at(aggregate, input.seq);
                            let matches = stored
                                .as_ref()
                                .map(|s| {
                                    s.id == event.id.0
                                        && s.r#type == versioned_type(&definition.r#type, durable.version)
                                        && s.data == encoded
                                })
                                .unwrap_or(false);
                            if matches {
                                if input.owner_id.is_some()
                                    && row.as_ref().and_then(|(_, owner)| owner.as_ref()).is_none()
                                {
                                    tx.upsert_sequence(aggregate, latest, None, false)?;
                                }
                                return Ok(Box::new(None::<(String, i64)>) as Box<dyn std::any::Any + Send>);
                            }
                            return Err(InvalidDurableEventError::new(
                                event.r#type.clone(),
                                format!("Replay diverged at aggregate {aggregate} sequence {}", input.seq),
                            )
                            .into());
                        }
                        if let Some((_, Some(owner))) = &row {
                            if input.owner_id.as_ref() != Some(owner) {
                                return Ok(Box::new(None::<(String, i64)>) as Box<dyn std::any::Any + Send>);
                            }
                        }
                    }
                    let seq = input.as_ref().map(|i| i.seq).unwrap_or(latest + 1);
                    if input.is_some() && seq != latest + 1 {
                            return Err(InvalidDurableEventError::new(
                                event.r#type.clone(),
                                format!(
                                    "Sequence mismatch for aggregate {aggregate}: expected {}, got {seq}",
                                    latest + 1
                                ),
                            )
                            .into());
                    }
                    if tx.stored_event_by_id(&event.id.0).is_some() {
                        return Err(InvalidDurableEventError::new(
                            event.r#type.clone(),
                            format!("Event {} already exists at aggregate {aggregate}", event.id.0),
                        )
                        .into());
                    }
                    let committed_payload = Payload {
                        id: event.id.clone(),
                        metadata: event.metadata.clone(),
                        r#type: event.r#type.clone(),
                        durable: Some(DurableInfo {
                            aggregateID: aggregate.clone(),
                            seq,
                            version: durable.version,
                        }),
                        location: event.location.clone(),
                        data: encoded.clone(),
                    };
                    for projector in &projectors {
                        (projector)(&committed_payload).await;
                    }
                    if let Some(commit_hook) = commit_hook {
                        (commit_hook)(seq).await;
                    }
                    let set_owner = input.as_ref().and_then(|i| i.owner_id.clone()).is_some()
                        && row.as_ref().and_then(|(_, owner)| owner.as_ref()).is_none();
                    tx.upsert_sequence(
                        aggregate,
                        seq,
                        input.as_ref().and_then(|i| i.owner_id.clone()),
                        set_owner,
                    )?;
                    tx.insert_event(StoredEvent {
                        id: event.id.0.clone(),
                        aggregate_id: aggregate.clone(),
                        seq,
                        r#type: versioned_type(&definition.r#type, durable.version),
                        data: encoded,
                    })?;
                    Ok(Box::new(Some((aggregate.clone(), seq))) as Box<dyn std::any::Any + Send>)
                })
            }))
            .await
            .map_err(unbox_store_error)?;
        let committed: Option<(String, i64)> = *committed
            .downcast::<Option<(String, i64)>>()
            .map_err(|_| StoreError::new("unexpected transaction result type"))?;
        if committed.is_some() {
            self.wake_durable(&aggregate).await;
        }
        Ok(committed)
    }
}

/// Recover the bus's own event error from the store's boxed transaction error.
fn unbox_store_error(error: crate::durable::TransactionError) -> BusError {
    if let Some(err) = error.downcast_ref::<InvalidDurableEventError>() {
        BusError::InvalidDurable(err.clone())
    } else {
        BusError::Store(StoreError::new(error.to_string()))
    }
}

/// Forward stored events after `*sequence`, advancing it as rows are sent.
async fn forward_durable(
    me: &EventBus,
    aggregate_id: &str,
    tx: &mpsc::UnboundedSender<Payload>,
    sequence: &mut i64,
) -> bool {
    for stored in me.inner.store.read_after(aggregate_id, *sequence).await {
        match me.decode_stored(&stored) {
            Ok(payload) => {
                if tx.send(payload).is_err() {
                    return false;
                }
                *sequence = (*sequence).max(stored.seq);
            }
            Err(err) => {
                tracing::warn!("failed to decode stored event: {err}");
            }
        }
    }
    true
}

/// Handle returned by [`EventBus::listen`]; dropping it removes the listener.
pub struct Unsubscribe {
    listener: Listener,
}

impl Unsubscribe {
    /// Removes the listener.
    pub async fn unsubscribe(self) {
        // The listener is removed by identity when this handle is dropped; the
        // bus stores an Arc clone, so nothing further is needed here.
        drop(self.listener);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Durable;
    use serde_json::json;

    fn durable_definition() -> Definition {
        Definition::define(
            "session.created",
            Some(Durable {
                version: 1,
                aggregate: "sessionID".to_string(),
            }),
            vec!["sessionID".to_string()],
        )
    }

    #[tokio::test]
    async fn publish_and_subscribe_typed() {
        let bus = EventBus::in_memory();
        let definition = Definition::define("catalog.updated", None, vec![]);
        let mut receiver = bus.subscribe("catalog.updated").await;
        let payload = bus
            .publish(&definition, &Map::new(), &PublishOptions::default())
            .await
            .unwrap();
        assert_eq!(payload.r#type, "catalog.updated");
        let received = receiver.recv().await.unwrap();
        assert_eq!(received.id, payload.id);
    }

    #[tokio::test]
    async fn publish_and_listen_all() {
        let bus = EventBus::in_memory();
        let definition = Definition::define("catalog.updated", None, vec![]);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let listener: Listener = Arc::new(move |_event: &Payload| {
            let tx = tx.clone();
            Box::pin(async move {
                let _ = tx.send(());
            })
        });
        bus.listen(listener).await;
        bus.publish(&definition, &Map::new(), &PublishOptions::default())
            .await
            .unwrap();
        rx.recv().await.expect("listener ran");
    }

    #[tokio::test]
    async fn durable_commit_and_read_back() {
        let bus = EventBus::in_memory();
        let definition = durable_definition();
        bus.registry().register(std::slice::from_ref(&definition));
        let mut data = Map::new();
        data.insert("sessionID".to_string(), json!("ses_1"));
        let payload = bus
            .publish(&definition, &data, &PublishOptions::default())
            .await
            .unwrap();
        let durable = payload.durable.as_ref().expect("durable field");
        assert_eq!(durable.aggregateID, "ses_1");
        // The reference starts at sequence 0 (`latest = row?.seq ?? -1`).
        assert_eq!(durable.seq, 0);
        assert_eq!(bus.latest_sequence("ses_1").await, 0);

        // publishing again appends
        let payload2 = bus
            .publish(&definition, &data, &PublishOptions::default())
            .await
            .unwrap();
        assert_eq!(payload2.durable.as_ref().unwrap().seq, 1);
        assert_eq!(bus.latest_sequence("ses_1").await, 1);
    }

    #[tokio::test]
    async fn durable_stream_historical_and_live() {
        let bus = EventBus::in_memory();
        let definition = durable_definition();
        bus.registry().register(std::slice::from_ref(&definition));
        let mut data = Map::new();
        data.insert("sessionID".to_string(), json!("ses_1"));
        bus.publish(&definition, &data, &PublishOptions::default())
            .await
            .unwrap();

        let mut receiver = bus.durable("ses_1".to_string(), -1);
        let first = receiver.recv().await.unwrap();
        assert_eq!(first.durable.as_ref().unwrap().seq, 0);

        bus.publish(&definition, &data, &PublishOptions::default())
            .await
            .unwrap();
        let second = receiver.recv().await.unwrap();
        assert_eq!(second.durable.as_ref().unwrap().seq, 1);
    }

    #[tokio::test]
    async fn replay_divergence_detected() {
        let bus = EventBus::in_memory();
        let definition = durable_definition();
        bus.registry().register(std::slice::from_ref(&definition));
        let mut data = Map::new();
        data.insert("sessionID".to_string(), json!("ses_1"));
        bus.publish(&definition, &data, &PublishOptions::default())
            .await
            .unwrap();

        // Same sequence as the committed event but a different id: diverged.
        let replay_event = SerializedEvent {
            id: EventId("evt_x".to_string()),
            r#type: "session.created.1".to_string(),
            seq: 0,
            aggregateID: "ses_1".to_string(),
            data: Map::from_iter([("sessionID".to_string(), json!("ses_1"))]),
        };
        let err = bus
            .replay(replay_event, &ReplayOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, BusError::InvalidDurable(_)));
    }

    #[tokio::test]
    async fn replay_idempotent_when_identical() {
        let bus = EventBus::in_memory();
        let definition = durable_definition();
        bus.registry().register(std::slice::from_ref(&definition));
        let mut data = Map::new();
        data.insert("sessionID".to_string(), json!("ses_1"));
        let payload = bus
            .publish(&definition, &data, &PublishOptions::default())
            .await
            .unwrap();
        let durable = payload.durable.as_ref().unwrap();

        // Replaying the exact stored event is a no-op (not an error).
        let replay_event = SerializedEvent {
            id: payload.id.clone(),
            r#type: "session.created.1".to_string(),
            seq: durable.seq,
            aggregateID: "ses_1".to_string(),
            data: data.clone(),
        };
        bus.replay(replay_event, &ReplayOptions::default())
            .await
            .expect("idempotent replay");
        assert_eq!(bus.latest_sequence("ses_1").await, 0);
    }
}
