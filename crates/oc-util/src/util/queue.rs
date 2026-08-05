use std::collections::VecDeque;
/// From reference/packages/opencode/src/util/queue.ts
///
/// `AsyncQueue` is a FIFO of pushed items consumed by concurrent `next`
/// callers. `work` mirrors the reference worker pool: `concurrency` tasks each
/// pop from the *end* of the pending list.
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct AsyncQueue<T> {
    queue: Mutex<VecDeque<T>>,
    waiters: Mutex<VecDeque<mpsc::UnboundedSender<T>>>,
}

impl<T> Default for AsyncQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AsyncQueue<T> {
    pub fn new() -> Self {
        AsyncQueue {
            queue: Mutex::new(VecDeque::new()),
            waiters: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push(&self, item: T) {
        let waiter = self.waiters.lock().expect("queue poisoned").pop_front();
        if let Some(waiter) = waiter {
            let _ = waiter.send(item);
        } else {
            self.queue.lock().expect("queue poisoned").push_back(item);
        }
    }

    pub async fn next(&self) -> T {
        if let Some(item) = self.queue.lock().expect("queue poisoned").pop_front() {
            return item;
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.waiters.lock().expect("queue poisoned").push_back(tx);
        rx.recv().await.expect("queue closed")
    }

    pub fn len(&self) -> usize {
        self.queue.lock().expect("queue poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub async fn work<T, F, Fut>(concurrency: usize, items: Vec<T>, f: F)
where
    T: Send + 'static,
    F: Fn(T) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let pending: Arc<Mutex<Vec<T>>> = Arc::new(Mutex::new(items));
    let mut tasks = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let pending = Arc::clone(&pending);
        let f = f.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                let item = pending.lock().expect("queue poisoned").pop();
                match item {
                    Some(item) => f(item).await,
                    None => return,
                }
            }
        }));
    }
    for task in tasks {
        task.await.expect("worker panicked");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn queue_delivers_in_fifo_order() {
        let queue = Arc::new(AsyncQueue::new());
        for i in 0..3 {
            queue.push(i);
        }
        for i in 0..3 {
            assert_eq!(queue.next().await, i);
        }
    }

    #[tokio::test]
    async fn queue_resolves_pending_waiters() {
        let queue = Arc::new(AsyncQueue::new());
        let next = tokio::spawn({
            let queue = Arc::clone(&queue);
            async move { queue.next().await }
        });
        tokio::task::yield_now().await;
        queue.push(99);
        assert_eq!(next.await.unwrap(), 99);
    }

    #[tokio::test]
    async fn work_processes_all_items() {
        let processed: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
        work(3, vec![1, 2, 3, 4, 5, 6], {
            let processed = Arc::clone(&processed);
            move |item| {
                let processed = Arc::clone(&processed);
                async move {
                    processed.lock().unwrap().push(item);
                }
            }
        })
        .await;
        let mut items = processed.lock().unwrap().clone();
        items.sort();
        assert_eq!(items, vec![1, 2, 3, 4, 5, 6]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn work_runs_concurrently() {
        let active: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let max: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        work(4, (0..8).collect(), {
            let active = Arc::clone(&active);
            let max = Arc::clone(&max);
            move |_| {
                let active = Arc::clone(&active);
                let max = Arc::clone(&max);
                async move {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })
        .await;
        assert_eq!(max.load(Ordering::SeqCst), 4);
    }
}
