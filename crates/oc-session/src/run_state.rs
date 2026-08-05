/// From reference/packages/opencode/src/effect/runner.ts and
/// reference/packages/opencode/src/session/run-state.ts
///
/// Per-session run concurrency: at most one running LLM turn or shell per
/// session. `ensure_running` joins existing runs or queues a run behind a
/// shell; `start_shell` fails with [`Busy`] when the session is active.
///
/// TODO(integration): oc-session-runner owns `effect/runner`; this is the
/// SessionRunState service adapted to tokio.
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex, Notify, OnceCell};

use crate::v1::WithParts;

#[derive(Debug, Clone, thiserror::Error)]
#[error("RunnerCancelled")]
pub struct Cancelled;

#[derive(Debug, Clone, thiserror::Error)]
#[error("RunnerBusy")]
pub struct Busy;

struct RunHandle {
    done: OnceCell<Result<WithParts, String>>,
    notify: Arc<Notify>,
    task: StdMutex<Option<tokio::task::JoinHandle<()>>>,
}

struct ShellHandle {
    task: tokio::task::JoinHandle<()>,
    result: Arc<OnceCell<Result<WithParts, String>>>,
    notify: Arc<Notify>,
}

enum RunState {
    Idle,
    Running(Arc<RunHandle>),
    Shell(Arc<ShellHandle>),
    ShellThenRun(Arc<ShellHandle>, Arc<RunHandle>),
}

/// Port of `Runner.make` on top of tokio.
pub struct Runner {
    state: Mutex<RunState>,
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            state: Mutex::new(RunState::Idle),
        }
    }
}

impl Runner {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn is_busy(&self) -> bool {
        let state = self.state.lock().await;
        !matches!(*state, RunState::Idle)
    }

    /// `Runner.ensureRunning` — start the work if idle, otherwise join the
    /// in-flight turn.
    pub async fn ensure_running(
        &self,
        work: impl Future<Output = Result<WithParts, String>> + Send + 'static,
    ) -> Result<WithParts, String> {
        let mut state = self.state.lock().await;
        let done = match &*state {
            RunState::Idle => {
                let handle = start_run(work);
                *state = RunState::Running(handle.clone());
                handle
            }
            RunState::Running(run) => run.clone(),
            RunState::ShellThenRun(_, run) => run.clone(),
            RunState::Shell(shell) => {
                let run = start_run(work);
                *state = RunState::ShellThenRun(shell.clone(), run.clone());
                run
            }
        };
        drop(state);
        await_done(done).await
    }

    /// `Runner.startShell` — only allowed when idle.
    pub async fn start_shell(
        &self,
        work: impl Future<Output = WithParts> + Send + 'static,
    ) -> Result<WithParts, Busy> {
        let mut state = self.state.lock().await;
        if !matches!(*state, RunState::Idle) {
            return Err(Busy);
        }
        let result = Arc::new(OnceCell::new());
        let notify = Arc::new(Notify::new());
        let result_clone = result.clone();
        let notify_clone = notify.clone();
        let task = tokio::spawn(async move {
            let outcome = Ok::<WithParts, String>(work.await);
            let _ = result_clone.set(outcome);
            notify_clone.notify_waiters();
        });
        let shell = Arc::new(ShellHandle {
            task,
            result,
            notify,
        });
        *state = RunState::Shell(shell.clone());
        drop(state);
        shell.notify.notified().await;
        match shell.result.get() {
            Some(result) => result.clone().map_err(|_| Busy),
            None => Err(Busy),
        }
    }

    /// `Runner.cancel` — interrupt any in-flight work.
    pub async fn cancel(&self) {
        let mut state = self.state.lock().await;
        match &*state {
            RunState::Idle => {}
            RunState::Running(run) => {
                if let Some(task) = run.task.lock().unwrap().take() {
                    task.abort();
                }
                run.notify.notify_waiters();
                *state = RunState::Idle;
            }
            RunState::Shell(shell) => {
                shell.task.abort();
                shell.notify.notify_waiters();
                *state = RunState::Idle;
            }
            RunState::ShellThenRun(shell, run) => {
                shell.task.abort();
                if let Some(task) = run.task.lock().unwrap().take() {
                    task.abort();
                }
                shell.notify.notify_waiters();
                run.notify.notify_waiters();
                *state = RunState::Idle;
            }
        }
    }
}

fn start_run(
    work: impl Future<Output = Result<WithParts, String>> + Send + 'static,
) -> Arc<RunHandle> {
    let handle = Arc::new(RunHandle {
        done: OnceCell::new(),
        notify: Arc::new(Notify::new()),
        task: StdMutex::new(None),
    });
    let handle_clone = handle.clone();
    let task = tokio::spawn(async move {
        let result = work.await;
        let _ = handle_clone.done.set(result);
        handle_clone.notify.notify_waiters();
    });
    *handle.task.lock().unwrap() = Some(task);
    handle
}

async fn await_done(run: Arc<RunHandle>) -> Result<WithParts, String> {
    run.notify.notified().await;
    match run.done.get() {
        Some(result) => result.clone(),
        None => Err(Cancelled.to_string()),
    }
}

/// From reference `run-state.ts:SessionRunState` — registry of per-session
/// runners.
#[derive(Default)]
pub struct SessionRunState {
    runners: Mutex<HashMap<String, Arc<Runner>>>,
}

impl SessionRunState {
    pub fn new() -> Self {
        Self::default()
    }

    async fn runner(&self, session_id: &str) -> Arc<Runner> {
        let mut runners = self.runners.lock().await;
        runners
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(Runner::new()))
            .clone()
    }

    pub async fn assert_not_busy(&self, session_id: &str) -> Result<(), crate::session::BusyError> {
        let runner = self.runner(session_id).await;
        if runner.is_busy().await {
            return Err(crate::session::BusyError {
                session_id: session_id.to_string(),
            });
        }
        Ok(())
    }

    pub async fn cancel(&self, session_id: &str) {
        let runner = self.runner(session_id).await;
        runner.cancel().await;
        let mut runners = self.runners.lock().await;
        runners.remove(session_id);
    }

    pub async fn ensure_running(
        &self,
        session_id: &str,
        work: impl Future<Output = Result<WithParts, String>> + Send + 'static,
    ) -> Result<WithParts, String> {
        let runner = self.runner(session_id).await;
        runner.ensure_running(work).await
    }

    pub async fn start_shell(
        &self,
        session_id: &str,
        work: impl Future<Output = WithParts> + Send + 'static,
    ) -> Result<WithParts, Busy> {
        let runner = self.runner(session_id).await;
        runner.start_shell(work).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ensure_running_runs_once_when_idle() {
        let runner = Runner::new();
        let result = runner
            .ensure_running(async {
                Ok::<WithParts, String>(WithParts {
                    info: crate::v1::Info::User(crate::v1::User {
                        id: "m".into(),
                        session_id: "s".into(),
                        role: "user".into(),
                        time: crate::v1::UserTime { created: 0 },
                        format: None,
                        summary: None,
                        agent: "primary".into(),
                        model: crate::v1::UserModel {
                            provider_id: "p".into(),
                            model_id: "m".into(),
                            variant: None,
                        },
                        system: None,
                        tools: None,
                    }),
                    parts: vec![],
                })
            })
            .await
            .unwrap();
        assert_eq!(result.info.id(), "m");
    }

    #[tokio::test]
    async fn start_shell_rejects_when_busy() {
        let runner = Runner::new();
        let _ = runner
            .start_shell(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                WithParts {
                    info: crate::v1::Info::User(empty_user()),
                    parts: vec![],
                }
            })
            .await;
        let err = runner
            .start_shell(async {
                WithParts {
                    info: crate::v1::Info::User(empty_user()),
                    parts: vec![],
                }
            })
            .await;
        assert!(matches!(err, Err(Busy)));
    }

    fn empty_user() -> crate::v1::User {
        crate::v1::User {
            id: String::new(),
            session_id: String::new(),
            role: "user".into(),
            time: crate::v1::UserTime { created: 0 },
            format: None,
            summary: None,
            agent: String::new(),
            model: crate::v1::UserModel {
                provider_id: String::new(),
                model_id: String::new(),
                variant: None,
            },
            system: None,
            tools: None,
        }
    }
}
