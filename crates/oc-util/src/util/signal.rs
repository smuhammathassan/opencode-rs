/// From reference/packages/opencode/src/util/signal.ts
///
/// A one-shot async signal. `trigger` resolves every current and future
/// `wait`; `wait` resolves once the signal has been triggered (mirrors a
/// `Promise` that can be awaited multiple times).
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::Notify;

pub struct Signal {
    aborted: AtomicBool,
    notify: Notify,
}

impl Signal {
    pub fn new() -> Arc<Self> {
        Arc::new(Signal {
            aborted: AtomicBool::new(false),
            notify: Notify::new(),
        })
    }

    pub fn trigger(&self) {
        self.aborted.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    pub async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.aborted.load(Ordering::SeqCst) {
                return;
            }
            notified.as_mut().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_resolves_after_trigger() {
        let signal = Signal::new();
        let handle = tokio::spawn({
            let signal = Arc::clone(&signal);
            async move {
                signal.wait().await;
                signal.aborted()
            }
        });
        signal.trigger();
        assert!(handle.await.unwrap());
    }

    #[tokio::test]
    async fn wait_resolves_if_already_triggered() {
        let signal = Signal::new();
        signal.trigger();
        signal.wait().await;
        assert!(signal.aborted());
    }

    #[tokio::test]
    async fn multiple_waits_all_resolve() {
        let signal = Signal::new();
        let signal_a = Arc::clone(&signal);
        let signal_b = Arc::clone(&signal);
        let a = tokio::spawn(async move { signal_a.wait().await });
        let b = tokio::spawn(async move { signal_b.wait().await });
        signal.trigger();
        a.await.unwrap();
        b.await.unwrap();
    }
}
