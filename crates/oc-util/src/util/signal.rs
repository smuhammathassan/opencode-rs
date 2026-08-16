/// From reference/packages/opencode/src/util/signal.ts
///
/// A one-shot async signal. `trigger` resolves every current and future
/// `wait`; `wait` resolves once the signal has been triggered (mirrors a
/// `Promise` that can be awaited multiple times).
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock,
};
use tokio::sync::Notify;

#[derive(Debug)]
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

static PROCESS_SHUTDOWN: OnceLock<Arc<Signal>> = OnceLock::new();

/// Return the process-wide shutdown signal used by long-running commands and
/// server listeners. Keeping the signal separate from the OS listener makes
/// embedded servers and tests able to inject a deterministic shutdown event.
pub fn process_shutdown() -> Arc<Signal> {
    PROCESS_SHUTDOWN.get_or_init(Signal::new).clone()
}

/// Wait for the signals that should gracefully stop the current process.
///
/// Unix handles both SIGINT and SIGTERM explicitly. Windows has no portable
/// SIGTERM equivalent, so Ctrl-C is the supported console shutdown event.
pub async fn wait_for_process_signal() {
    #[cfg(unix)]
    {
        let mut sigint =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) {
                Ok(signal) => signal,
                Err(error) => {
                    tracing::warn!(?error, "failed to install SIGINT handler");
                    return;
                }
            };
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    tracing::warn!(?error, "failed to install SIGTERM handler");
                    return;
                }
            };

        tokio::select! {
            _ = sigint.recv() => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(windows)]
    {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(?error, "failed to install Ctrl-C handler");
        }
    }
}

/// Start forwarding the first process signal into [`process_shutdown`].
///
/// The returned task is owned by the CLI runtime and should be aborted when
/// command dispatch finishes normally.
pub fn spawn_process_signal_handler() -> tokio::task::JoinHandle<()> {
    let shutdown = process_shutdown();
    tokio::spawn(async move {
        wait_for_process_signal().await;
        shutdown.trigger();
    })
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
