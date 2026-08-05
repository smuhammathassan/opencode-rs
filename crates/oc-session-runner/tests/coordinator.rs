//! `RunCoordinator` concurrency semantics: per-key serialization, cross-key
//! parallelism, wake coalescing, and interruption.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use oc_session_runner::run_coordinator::RunCoordinator;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serializes_same_key_and_joins_waiters() {
    let runs = Arc::new(AtomicUsize::new(0));
    let drain_runs = runs.clone();
    let coordinator = RunCoordinator::new(move |key: &'static str, force: bool, _token| {
        let drain_runs = drain_runs.clone();
        async move {
            assert!(force);
            drain_runs.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            let _ = key;
            Ok::<(), ()>(())
        }
    });

    let (a, b) = tokio::join!(coordinator.run("s1"), coordinator.run("s1"));
    assert!(a.is_ok());
    assert!(b.is_ok());
    assert_eq!(runs.load(Ordering::SeqCst), 1, "same key drains once");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn different_keys_run_concurrently() {
    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let shared = (concurrent.clone(), max_concurrent.clone());
    let coordinator = RunCoordinator::new(move |key: &'static str, _force, _token| {
        let (concurrent, max_concurrent) = shared.clone();
        async move {
            let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            max_concurrent.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            concurrent.fetch_sub(1, Ordering::SeqCst);
            let _ = key;
            Ok::<(), ()>(())
        }
    });

    let (a, b) = tokio::join!(coordinator.run("s1"), coordinator.run("s2"));
    assert!(a.is_ok() && b.is_ok());
    assert_eq!(
        max_concurrent.load(Ordering::SeqCst),
        2,
        "different keys drain in parallel"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wake_after_success_restarts_drain() {
    let runs = Arc::new(AtomicUsize::new(0));
    let drain_runs = runs.clone();
    let coordinator = RunCoordinator::new(move |key: &'static str, force: bool, _token| {
        let drain_runs = drain_runs.clone();
        async move {
            let count = drain_runs.fetch_add(1, Ordering::SeqCst) + 1;
            if count == 1 {
                assert!(force, "first drain is forced");
            } else {
                assert!(!force, "wake restarts with force=false");
            }
            // Keep the first drain alive so both wake calls coalesce onto it.
            tokio::time::sleep(Duration::from_millis(40)).await;
            let _ = key;
            Ok::<(), ()>(())
        }
    });

    let handle = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("s1").await }
    });
    // Let the first drain start, then register coalesced wakes.
    tokio::time::sleep(Duration::from_millis(20)).await;
    coordinator.wake("s1").await;
    coordinator.wake("s1").await;
    assert!(handle.await.unwrap().is_ok());

    // Wait for the successor drain to complete.
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        runs.load(Ordering::SeqCst),
        2,
        "pending wake coalesces into one successor"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupt_stops_drain_cooperatively() {
    let started = Arc::new(tokio::sync::Notify::new());
    let started_in_drain = started.clone();
    let coordinator = RunCoordinator::new(move |key: &'static str, _force, token| {
        let started = started_in_drain.clone();
        async move {
            let _ = key;
            started.notify_one();
            tokio::select! {
                _ = token.cancelled() => Ok::<(), ()>(()),
                _ = tokio::time::sleep(Duration::from_millis(500)) => Ok(()),
            }
        }
    });

    let handle = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("s1").await }
    });
    started.notified().await;
    coordinator.interrupt("s1").await;
    assert!(handle.await.unwrap().is_ok());
    assert!(coordinator.active().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_drain_with_wake_hands_off_to_successor() {
    let runs = Arc::new(AtomicUsize::new(0));
    let drain_runs = runs.clone();
    let coordinator = RunCoordinator::new(move |key: &'static str, _force, _token| {
        let drain_runs = drain_runs.clone();
        async move {
            let count = drain_runs.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = key;
            if count == 1 {
                Err::<(), &str>("boom")
            } else {
                Ok(())
            }
        }
    });

    let handle = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("s1").await }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    coordinator.wake("s1").await;

    let result = handle.await.unwrap();
    assert!(result.is_err(), "first drain fails");
    // The successor was started for the pending wake; wait for it to settle.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}
