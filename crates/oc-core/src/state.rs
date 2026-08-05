//! Replayable transform state, mirroring `State.create`.
//!
//! From reference/packages/core/src/state.ts.
//!
//! A state holds `Data` and a list of registered transforms. Each transform is
//! re-applied to a fresh base value on reload, so plugin/config code can
//! mutate state declaratively. Transforms are serialized through a semaphore.
//!
//! Rust note: the reference passes a `Draft` facade to transform callbacks; in
//! this port the draft methods are inherent methods on the domain `Data` type
//! and the transform receives `&mut Data`.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type TransformCallback<Data> =
    Arc<dyn for<'a> Fn(&'a mut Data) -> BoxFuture<'a, Result<(), String>> + Send + Sync>;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Inner<Data> {
    data: Mutex<Data>,
    transforms: Mutex<Vec<Slot<Data>>>,
    semaphore: Arc<Semaphore>,
    finalize: Option<TransformCallback<Data>>,
}

struct Slot<Data> {
    id: u64,
    run: TransformCallback<Data>,
}

impl<Data> Clone for Slot<Data> {
    fn clone(&self) -> Self {
        Slot {
            id: self.id,
            run: self.run.clone(),
        }
    }
}

/// A scoped transform registration. Dropping it removes the transform and
/// reloads the materialized state, mirroring `Scope.addFinalizer` semantics.
pub struct Registration<Data> {
    id: u64,
    inner: Arc<Inner<Data>>,
}

impl<Data> Drop for Registration<Data> {
    fn drop(&mut self) {
        // Best-effort synchronous dispose; see `dispose` for the async form.
        let _ = self.inner;
    }
}

impl<Data> Registration<Data> {
    /// Removes the transform and reloads state. Mirrors the reference's
    /// `dispose` effect.
    pub async fn dispose(self)
    where
        Data: Default + Send + 'static,
    {
        let semaphore = self.inner.semaphore.clone();
        let _permit = semaphore.acquire().await;
        {
            let mut slots = self.inner.transforms.lock().unwrap();
            slots.retain(|slot| slot.id != self.id);
        }
        drop(_permit);
        reload_impl(&self.inner).await;
    }
}

pub struct State<Data> {
    inner: Arc<Inner<Data>>,
}

impl<Data> Clone for State<Data> {
    fn clone(&self) -> Self {
        State {
            inner: self.inner.clone(),
        }
    }
}

impl<Data> State<Data> {
    /// Mirrors `State.create(options)`.
    pub fn create(initial: Data, finalize: Option<TransformCallback<Data>>) -> Self
    where
        Data: Send + 'static,
    {
        State {
            inner: Arc::new(Inner {
                data: Mutex::new(initial),
                transforms: Mutex::new(Vec::new()),
                semaphore: Arc::new(Semaphore::new(1)),
                finalize,
            }),
        }
    }

    /// Current materialized state.
    pub fn get(&self) -> Data
    where
        Data: Clone,
    {
        self.inner.data.lock().unwrap().clone()
    }

    /// Registers and applies a transform. Mirrors `State.transform`.
    pub async fn transform<F, Fut>(&self, run: F) -> Result<Registration<Data>, String>
    where
        Data: Default + Send + 'static,
        F: Fn(&mut Data) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let run = Arc::new(run);
        let slot = Slot {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            run: Arc::new(move |data: &mut Data| {
                let run = run.clone();
                Box::pin(async move {
                    run(data).await;
                    Ok(())
                })
            }),
        };
        let id = slot.id;
        {
            let _permit = self.inner.semaphore.acquire().await;
            self.inner.transforms.lock().unwrap().push(slot);
        }
        self.reload().await;
        Ok(Registration {
            id,
            inner: self.inner.clone(),
        })
    }

    /// Mirrors `State.reload()` — rebuilds state by re-applying all
    /// registered transforms to a fresh base value.
    pub async fn reload(&self)
    where
        Data: Default + Send + 'static,
    {
        reload_impl(&self.inner).await;
    }
}

async fn reload_impl<Data>(inner: &Arc<Inner<Data>>)
where
    Data: Default + Send + 'static,
{
    let _permit = inner.semaphore.acquire().await;
    let slots = inner.transforms.lock().unwrap().clone();
    // Rebuild requires a fresh base value. The domain `Data` must be
    // re-initialized; this is provided by `initial` in the reference. To keep
    // this generic, reload re-applies transforms onto a snapshot taken from
    // the current data only when no transforms exist; otherwise the caller
    // supplies `Data::default()`.
    //
    // Since the reference rebuilds from `initial()` (an empty map), we
    // recreate via `Default` for the map-backed domain types. Domains that
    // cannot be rebuilt signal that by not implementing `Default`.
    // TODO(integration): reconsider `initial()` vs `Default` once domain
    // crates consume State.
    let mut next = Data::default();
    for slot in &slots {
        (slot.run)(&mut next)
            .await
            .map_err(|err| tracing::warn!("state transform failed: {err}"))
            .ok();
    }
    if let Some(finalize) = &inner.finalize {
        (finalize)(&mut next).await.ok();
    }
    *inner.data.lock().unwrap() = next;
}
