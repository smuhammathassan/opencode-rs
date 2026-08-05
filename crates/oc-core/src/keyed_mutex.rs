//! In-memory keyed mutex: one lock per key.
//!
//! From reference/packages/core/src/effect/keyed-mutex.ts.
//! Entries are removed when no holder or waiter remains.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;

pub struct KeyedMutex<K> {
    inner: Mutex<HashMap<K, Entry>>,
}

struct Entry {
    semaphore: Arc<Semaphore>,
    users: usize,
}

impl<K> KeyedMutex<K>
where
    K: Eq + std::hash::Hash + Clone,
{
    pub fn make() -> Self {
        KeyedMutex {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Number of distinct keys currently tracked. Mirrors `KeyedMutex.size`.
    pub fn size(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// Mirrors `KeyedMutex.withLock(key)(effect)`: serializes same-key
    /// futures and runs different keys independently.
    pub async fn with_lock<F, T>(&self, key: K, effect: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let entry = {
            let mut locks = self.inner.lock().unwrap();
            let entry = locks.entry(key.clone()).or_insert_with(|| Entry {
                semaphore: Arc::new(Semaphore::new(1)),
                users: 0,
            });
            entry.users += 1;
            entry.semaphore.clone()
        };
        let result = {
            let _permit = entry.acquire().await;
            effect.await
        };
        let mut locks = self.inner.lock().unwrap();
        if let Some(entry) = locks.get_mut(&key) {
            entry.users -= 1;
            if entry.users == 0 {
                locks.remove(&key);
            }
        }
        result
    }
}
