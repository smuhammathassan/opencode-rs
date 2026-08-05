//! Process-local background job registry.
//!
//! From reference/packages/core/src/background-job.ts.
//!
//! Entries are intentionally not durable: process restart loses status and
//! interrupts live work (same caveat as the reference).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

use crate::id;
use crate::state::BoxFuture;

/// `BackgroundJob.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Completed,
    Error,
    Cancelled,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Completed => "completed",
            Status::Error => "error",
            Status::Cancelled => "cancelled",
        }
    }
}

/// `BackgroundJob.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Info {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub status: String,
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

pub type Run = Arc<dyn Fn() -> BoxFuture<'static, Result<String, String>> + Send + Sync>;

pub struct StartInput {
    pub id: Option<String>,
    pub r#type: String,
    pub title: Option<String>,
    pub metadata: Option<Map<String, Value>>,
    pub on_promote: Option<BoxFuture<'static, ()>>,
    pub run: Run,
}

pub struct ExtendInput {
    pub id: String,
    pub run: Run,
}

pub struct WaitInput {
    pub id: String,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug)]
pub struct WaitResult {
    pub info: Option<Info>,
    pub timed_out: bool,
}

struct Job {
    info: Info,
    done_rx: Mutex<Option<oneshot::Receiver<Info>>>,
    done_tx: Mutex<Option<oneshot::Sender<Info>>>,
    promoted_rx: Mutex<Option<oneshot::Receiver<Info>>>,
    promoted_tx: Mutex<Option<oneshot::Sender<Info>>>,
    /// Receiver side of the current run segment's tail; the next extended
    /// segment awaits it before starting. The matching sender is owned by the
    /// segment's task.
    tail: Mutex<Option<oneshot::Receiver<()>>>,
    pending: usize,
    next: u64,
    output: Option<(u64, String)>,
    token: u64,
    on_promote: Option<BoxFuture<'static, ()>>,
}
/// The background job service.
#[derive(Clone)]
pub struct BackgroundJob {
    jobs: Arc<Mutex<HashMap<String, Job>>>,
    next_token: Arc<std::sync::atomic::AtomicU64>,
}

impl BackgroundJob {
    pub fn new() -> Self {
        BackgroundJob {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_token: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub async fn list(&self) -> Vec<Info> {
        let jobs = self.jobs.lock().unwrap();
        let mut infos: Vec<Info> = jobs.values().map(|job| job.info.clone()).collect();
        drop(jobs);
        infos.sort_by_key(|info| info.started_at);
        infos
    }

    pub async fn get(&self, id: &str) -> Option<Info> {
        self.jobs
            .lock()
            .unwrap()
            .get(id)
            .map(|job| job.info.clone())
    }

    /// `start(input)`.
    pub async fn start(&self, input: StartInput) -> Info {
        // Reuse an existing running job with the same id.
        {
            let jobs = self.jobs.lock().unwrap();
            if let Some(existing) = jobs.get(input.id.as_deref().unwrap_or("")) {
                if existing.info.status == "running" {
                    return existing.info.clone();
                }
            }
        }
        let id = input.id.clone().unwrap_or_else(|| {
            id::ascending("job", None).unwrap_or_else(|_| format!("job_{}", std::process::id()))
        });
        let started_at = Self::now();
        let (done_tx, done_rx) = oneshot::channel();
        let (promoted_tx, promoted_rx) = oneshot::channel();
        let (tail_tx, tail_rx) = oneshot::channel();
        let token = self
            .next_token
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let job = Job {
            info: Info {
                id: id.clone(),
                r#type: input.r#type.clone(),
                title: input.title.clone(),
                status: "running".to_string(),
                started_at,
                completed_at: None,
                output: None,
                error: None,
                metadata: input.metadata.clone(),
            },
            done_rx: Mutex::new(Some(done_rx)),
            done_tx: Mutex::new(Some(done_tx)),
            promoted_rx: Mutex::new(Some(promoted_rx)),
            promoted_tx: Mutex::new(Some(promoted_tx)),
            tail: Mutex::new(Some(tail_rx)),
            pending: 1,
            next: 1,
            output: None,
            token,
            on_promote: input.on_promote,
        };
        let snapshot = job.info.clone();
        self.jobs.lock().unwrap().insert(id.clone(), job);

        let me = self.clone();
        tokio::spawn(async move {
            let result = (input.run)().await;
            me.settle(&id, token, 0, result).await;
            let _ = tail_tx.send(());
        });
        snapshot
    }

    /// `extend(input)` — chains an extra run segment onto a running job.
    pub async fn extend(&self, input: ExtendInput) -> bool {
        let (previous_tail, tail_sender, sequence, token) = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(&input.id) else {
                return false;
            };
            if job.info.status != "running" {
                return false;
            }
            let (tail_tx, tail_rx) = oneshot::channel();
            let sequence = job.next;
            let token = job.token;
            let previous = {
                let mut tail = job.tail.lock().unwrap();
                tail.take()
            };
            {
                let mut tail = job.tail.lock().unwrap();
                *tail = Some(tail_rx);
            }
            job.pending += 1;
            job.next += 1;
            (previous, tail_tx, sequence, token)
        };
        let me = self.clone();
        let id = input.id.clone();
        tokio::spawn(async move {
            if let Some(receiver) = previous_tail {
                let _ = receiver.await;
            }
            let result = (input.run)().await;
            me.settle(&id, token, sequence, result).await;
            let _ = tail_sender.send(());
        });
        true
    }

    /// `wait(input)`.
    pub async fn wait(&self, input: WaitInput) -> WaitResult {
        let receiver = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(&input.id) else {
                return WaitResult {
                    info: None,
                    timed_out: false,
                };
            };
            if job.info.status != "running" {
                return WaitResult {
                    info: Some(job.info.clone()),
                    timed_out: false,
                };
            }
            let taken = job.done_rx.lock().unwrap().take();
            taken
        };
        let snapshot = {
            let jobs = self.jobs.lock().unwrap();
            jobs.get(&input.id).map(|job| job.info.clone())
        };
        match (input.timeout_ms, receiver) {
            (None, Some(receiver)) => match receiver.await {
                Ok(info) => WaitResult {
                    info: Some(info),
                    timed_out: false,
                },
                Err(_) => WaitResult {
                    info: snapshot,
                    timed_out: false,
                },
            },
            (Some(0), _) => WaitResult {
                info: snapshot,
                timed_out: true,
            },
            (Some(ms), Some(receiver)) => {
                match timeout(Duration::from_millis(ms), receiver).await {
                    Ok(Ok(info)) => WaitResult {
                        info: Some(info),
                        timed_out: false,
                    },
                    _ => WaitResult {
                        info: snapshot,
                        timed_out: true,
                    },
                }
            }
            _ => WaitResult {
                info: snapshot,
                timed_out: false,
            },
        }
    }

    /// `waitForPromotion(id)`. Returns `None` when the job is unknown or no
    /// longer running (the reference awaits forever in that case).
    pub async fn wait_for_promotion(&self, id: &str) -> Option<Info> {
        let receiver = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(id) else {
                return None;
            };
            if job.info.status != "running" {
                return None;
            }
            if job
                .info
                .metadata
                .as_ref()
                .and_then(|m| m.get("background"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                return Some(job.info.clone());
            }
            let taken = job.promoted_rx.lock().unwrap().take();
            taken
        };
        let receiver = receiver?;
        receiver.await.ok()?;
        let jobs = self.jobs.lock().unwrap();
        jobs.get(id).map(|job| job.info.clone())
    }

    /// `promote(id)`.
    pub async fn promote(&self, id: &str) -> Option<Info> {
        let (info, promoted, on_promote) = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(id) else {
                return None;
            };
            if job.info.status != "running" {
                return Some(job.info.clone());
            }
            if job
                .info
                .metadata
                .as_ref()
                .and_then(|m| m.get("background"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                return Some(job.info.clone());
            }
            let metadata = job.info.metadata.get_or_insert_with(Default::default);
            metadata.insert("background".to_string(), Value::Bool(true));
            let snapshot = job.info.clone();
            let promoted = job.promoted_tx.lock().unwrap().take();
            let on_promote = job.on_promote.take();
            (snapshot, promoted, on_promote)
        };
        if let Some(sender) = promoted {
            let _ = sender.send(info.clone());
        }
        if let Some(on_promote) = on_promote {
            on_promote.await;
        }
        Some(info)
    }

    /// `cancel(id)`.
    pub async fn cancel(&self, id: &str) -> Option<Info> {
        let completed_at = Self::now();
        let (info, done) = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(id) else {
                return None;
            };
            if job.info.status != "running" {
                return Some(job.info.clone());
            }
            job.info.status = "cancelled".to_string();
            job.info.completed_at = Some(completed_at);
            let snapshot = job.info.clone();
            let done = job.done_tx.lock().unwrap().take();
            (snapshot, done)
        };
        if let Some(sender) = done {
            let _ = sender.send(info.clone());
        }
        Some(info)
    }

    /// Mirrors `BackgroundJob.settle`. Returns the settled `Info`.
    async fn settle(
        &self,
        id: &str,
        token: u64,
        sequence: u64,
        exit: Result<String, String>,
    ) -> Option<Info> {
        let completed_at = Self::now();
        let (snapshot, done, promoted) = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(id) else {
                return None;
            };
            if job.token != token || job.info.status != "running" {
                return None;
            }
            job.pending -= 1;
            if let Ok(text) = &exit {
                if job
                    .output
                    .as_ref()
                    .map(|(seq, _)| sequence > *seq)
                    .unwrap_or(true)
                {
                    job.output = Some((sequence, text.clone()));
                }
            }
            if exit.is_ok() && job.pending > 0 {
                return None;
            }
            let status = if exit.is_ok() {
                Status::Completed
            } else {
                Status::Error
            };
            job.info.status = status.as_str().to_string();
            job.info.completed_at = Some(completed_at);
            if let Some((_, text)) = &job.output {
                job.info.output = Some(text.clone());
            }
            if let Err(error) = &exit {
                job.info.error = Some(error.clone());
            }
            let snapshot = job.info.clone();
            let done = job.done_tx.lock().unwrap().take();
            let promoted = job.promoted_tx.lock().unwrap().take();
            (snapshot, done, promoted)
        };
        if let Some(sender) = done {
            let _ = sender.send(snapshot.clone());
        }
        if let Some(sender) = promoted {
            let _ = sender.send(snapshot.clone());
        }
        Some(snapshot)
    }
}

impl Default for BackgroundJob {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_and_wait() {
        let jobs = BackgroundJob::new();
        let run: Run = Arc::new(|| Box::pin(async { Ok("done".to_string()) }));
        let info = jobs
            .start(StartInput {
                id: None,
                r#type: "test.job".to_string(),
                title: Some("a test".to_string()),
                metadata: None,
                on_promote: None,
                run,
            })
            .await;
        assert_eq!(info.status, "running");
        assert!(info.id.starts_with("job_"));
        let result = jobs
            .wait(WaitInput {
                id: info.id.clone(),
                timeout_ms: None,
            })
            .await;
        assert!(!result.timed_out);
        let info = result.info.unwrap();
        assert_eq!(info.status, "completed");
        assert_eq!(info.output.as_deref(), Some("done"));
        assert!(info.completed_at.is_some());
    }

    #[tokio::test]
    async fn error_job() {
        let jobs = BackgroundJob::new();
        let run: Run = Arc::new(|| Box::pin(async { Err("boom".to_string()) }));
        let info = jobs
            .start(StartInput {
                id: None,
                r#type: "test.job".to_string(),
                title: None,
                metadata: None,
                on_promote: None,
                run,
            })
            .await;
        let result = jobs
            .wait(WaitInput {
                id: info.id.clone(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(result.info.unwrap().status, "error");
    }

    #[tokio::test]
    async fn wait_timeout() {
        let jobs = BackgroundJob::new();
        let run: Run = Arc::new(|| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok("late".to_string())
            })
        });
        let info = jobs
            .start(StartInput {
                id: None,
                r#type: "test.job".to_string(),
                title: None,
                metadata: None,
                on_promote: None,
                run,
            })
            .await;
        let result = jobs
            .wait(WaitInput {
                id: info.id.clone(),
                timeout_ms: Some(10),
            })
            .await;
        assert!(result.timed_out);
        assert_eq!(result.info.unwrap().status, "running");
    }

    #[tokio::test]
    async fn promote_marks_background() {
        let jobs = BackgroundJob::new();
        let run: Run = Arc::new(|| Box::pin(async { Ok("x".to_string()) }));
        let info = jobs
            .start(StartInput {
                id: None,
                r#type: "test.job".to_string(),
                title: None,
                metadata: None,
                on_promote: None,
                run,
            })
            .await;
        let promoted = jobs.promote(&info.id).await.unwrap();
        assert_eq!(
            promoted
                .metadata
                .as_ref()
                .unwrap()
                .get("background")
                .unwrap(),
            &Value::Bool(true)
        );
        assert!(jobs.wait_for_promotion(&info.id).await.is_some());
    }

    #[tokio::test]
    async fn extend_chains_segments() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let jobs = BackgroundJob::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let run: Run = {
            let counter = counter.clone();
            Arc::new(move || {
                let counter = counter.clone();
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(format!("seg{}", counter.load(Ordering::SeqCst)))
                })
            })
        };
        let info = jobs
            .start(StartInput {
                id: None,
                r#type: "test.job".to_string(),
                title: None,
                metadata: None,
                on_promote: None,
                run: run.clone(),
            })
            .await;
        let id = info.id.clone();
        assert!(
            jobs.extend(ExtendInput {
                id: id.clone(),
                run
            })
            .await
        );
        let result = jobs
            .wait(WaitInput {
                id: id.clone(),
                timeout_ms: Some(2000),
            })
            .await;
        assert!(!result.timed_out, "job should complete with both segments");
        let info = result.info.unwrap();
        assert_eq!(info.status, "completed");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
