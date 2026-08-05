//! `SessionRunCoordinator` — serializes execution per key while allowing
//! different keys to run concurrently.
//!
//! Ports `packages/core/src/session/run-coordinator.ts`. The Effect
//! `Deferred`/`Fiber` machinery is modeled with `watch`-free result slots plus
//! a `Notify` for waiters, and a per-entry `CancellationToken` for cooperative
//! interruption. `E: Clone` is required so multiple concurrent `run` waiters
//! can each observe the drain's exit.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type DrainFn<K, E> = Arc<
    dyn Fn(K, bool, CancellationToken) -> Pin<Box<dyn Future<Output = Result<(), E>> + Send>>
        + Send
        + Sync,
>;

type ActiveEntries<K, E> = Arc<Mutex<HashMap<K, Arc<EntryState<E>>>>>;

struct EntryState<E> {
    result: Mutex<Option<Result<(), E>>>,
    notify: Notify,
    owner: Mutex<Option<JoinHandle<()>>>,
    cancel: Mutex<Option<CancellationToken>>,
    pending_wake: AtomicBool,
    stopping: AtomicBool,
}

impl<E> EntryState<E> {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            notify: Notify::new(),
            owner: Mutex::new(None),
            cancel: Mutex::new(None),
            pending_wake: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        }
    }
}

/// Serializes execution for each key while allowing different keys to run
/// concurrently. Clone is cheap (internals are shared) so callers can move a
/// handle into spawned tasks.
/// /// From reference/packages/core/src/session/run-coordinator.ts
#[derive(Clone)]
pub struct RunCoordinator<K, E> {
    active: Arc<Mutex<HashMap<K, Arc<EntryState<E>>>>>,
    drain: DrainFn<K, E>,
}

impl<K, E> RunCoordinator<K, E>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    E: Clone + Send + 'static,
{
    pub fn new<F, Fut>(drain: F) -> Self
    where
        F: Fn(K, bool, CancellationToken) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
    {
        let drain: DrainFn<K, E> =
            Arc::new(move |key, force, token| Box::pin(drain(key, force, token)));
        Self {
            active: Arc::new(Mutex::new(HashMap::new())),
            drain,
        }
    }

    /// Snapshots keys with an execution owned by this coordinator.
    /// /// From reference/packages/core/src/session/run-coordinator.ts
    pub async fn active(&self) -> HashSet<K> {
        self.active.lock().unwrap().keys().cloned().collect()
    }

    /// Starts execution while idle or joins the active execution.
    /// /// From reference/packages/core/src/session/run-coordinator.ts
    pub async fn run(&self, key: K) -> Result<(), E> {
        let current_key = key;
        loop {
            let entry = {
                let mut active = self.active.lock().unwrap();
                match active.get(&current_key) {
                    Some(entry) => entry.clone(),
                    None => {
                        let entry = Arc::new(EntryState::new());
                        active.insert(current_key.clone(), entry.clone());
                        drop(active);
                        start_entry(
                            &self.active,
                            &self.drain,
                            current_key.clone(),
                            entry.clone(),
                            true,
                        );
                        entry
                    }
                }
            };
            loop {
                let stopping = entry.stopping.load(Ordering::Acquire);
                let result = entry.result.lock().unwrap().clone();
                if let Some(result) = result {
                    // A stopping entry hands over to a fresh execution after it
                    // drains; mirrors `Deferred.await(...).pipe(andThen(run(key)))`.
                    if stopping {
                        break;
                    }
                    return result;
                }
                entry.notify.notified().await;
            }
        }
    }

    /// Registers one coalesced follow-up after newly recorded work.
    /// /// From reference/packages/core/src/session/run-coordinator.ts
    pub async fn wake(&self, key: K) {
        let mut active = self.active.lock().unwrap();
        if let Some(entry) = active.get(&key) {
            entry.pending_wake.store(true, Ordering::Release);
            return;
        }
        let entry = Arc::new(EntryState::new());
        active.insert(key.clone(), entry.clone());
        drop(active);
        start_entry(&self.active, &self.drain, key, entry, false);
    }

    /// Stops active execution and waits for its cleanup.
    /// /// From reference/packages/core/src/session/run-coordinator.ts
    pub async fn interrupt(&self, key: K) {
        let entry = self.active.lock().unwrap().get(&key).cloned();
        let Some(entry) = entry else { return };
        let owner = entry.owner.lock().unwrap().take();
        let Some(owner) = owner else { return };
        entry.stopping.store(true, Ordering::Release);
        entry.pending_wake.store(false, Ordering::Release);
        if let Some(cancel) = entry.cancel.lock().unwrap().clone() {
            cancel.cancel();
        }
        let _ = owner.await;
    }
}

/// Spawns the drain for an entry. The drain receives a fresh cancellation
/// token; interruption is cooperative (the drain must observe the token).
/// /// From reference/packages/core/src/session/run-coordinator.ts (`start`)
fn start_entry<K, E>(
    active: &ActiveEntries<K, E>,
    drain: &DrainFn<K, E>,
    key: K,
    entry: Arc<EntryState<E>>,
    force: bool,
) where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    E: Clone + Send + 'static,
{
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let active = active.clone();
    let drain = drain.clone();
    let task_key = key.clone();
    let entry_for_task = entry.clone();
    let handle = tokio::spawn(async move {
        let result = drain(task_key.clone(), force, token).await;
        settle(&active, &drain, task_key, entry_for_task, result);
    });
    *entry.owner.lock().unwrap() = Some(handle);
    *entry.cancel.lock().unwrap() = Some(cancel);
}

/// Completes an entry after its drain exits, restarting or handing off when a
/// wake was recorded. Mirrors `settle`.
/// /// From reference/packages/core/src/session/run-coordinator.ts (`settle`)
fn settle<K, E>(
    active: &ActiveEntries<K, E>,
    drain: &DrainFn<K, E>,
    key: K,
    entry: Arc<EntryState<E>>,
    result: Result<(), E>,
) where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    E: Clone + Send + 'static,
{
    let success = result.is_ok();
    let stopping = entry.stopping.load(Ordering::Acquire);
    let pending_wake = entry.pending_wake.swap(false, Ordering::AcqRel);

    if success && !stopping && pending_wake {
        // Drain succeeded and work arrived meanwhile: restart the same entry.
        start_entry(active, drain, key, entry, false);
        return;
    }

    if pending_wake {
        // Drain failed but work is pending: hand off to a successor.
        let successor = Arc::new(EntryState::new());
        start_entry(active, drain, key, successor, false);
    } else {
        let mut map = active.lock().unwrap();
        if map
            .get(&key)
            .map(|current| Arc::ptr_eq(current, &entry))
            .unwrap_or(false)
        {
            map.remove(&key);
        }
    }

    *entry.result.lock().unwrap() = Some(result);
    entry.notify.notify_waiters();
}
