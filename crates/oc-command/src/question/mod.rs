//! Interactive prompt plumbing: ask the user a question mid-session.
//!
//! From reference/packages/opencode/src/question/index.ts. The Effect
//! `Deferred` is ported with `tokio::sync::oneshot`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

pub mod schema;

pub use schema::{
    Answer, Info, Prompt, QuestionId, QuestionOption, Rejected, Replied, Reply, Request, Tool,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuestionError {
    #[error("The user dismissed this question")]
    Rejected,
    #[error("Question not found: {request_id}")]
    NotFound { request_id: QuestionId },
}

struct PendingEntry {
    info: Request,
    tx: tokio::sync::oneshot::Sender<Result<Vec<Answer>, QuestionError>>,
}

#[derive(Default)]
struct State {
    pending: HashMap<QuestionId, PendingEntry>,
}

/// A pending question awaiting resolution.
pub struct AskHandle(tokio::sync::oneshot::Receiver<Result<Vec<Answer>, QuestionError>>);

impl Future for AskHandle {
    type Output = Result<Vec<Answer>, QuestionError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx).map(|result| match result {
            Ok(inner) => inner,
            Err(_) => Err(QuestionError::Rejected),
        })
    }
}

/// The question service, mirroring the `Question` layer's `State`.
#[derive(Default)]
pub struct QuestionService {
    state: Mutex<State>,
}

impl QuestionService {
    pub fn new() -> Self {
        QuestionService::default()
    }

    /// Create a pending question and return it with a handle that resolves
    /// once the user replies or rejects.
    ///
    /// The reference `Question.ask` blocks on a `Deferred`; here the creation
    /// and the resolution handle are split so callers can register the pending
    /// request (visible via [`QuestionService::list`]) before awaiting.
    /// TODO(integration): use `oc-session`'s `SessionID` instead of `&str`.
    pub fn ask(
        &self,
        session_id: &str,
        questions: Vec<Info>,
        tool: Option<Tool>,
    ) -> (AskHandle, Request) {
        let id = QuestionId::ascending();
        tracing::info!(id = %id, questions = questions.len(), "asking");

        let (tx, rx) = tokio::sync::oneshot::channel();
        let info = Request {
            id: id.clone(),
            session_id: session_id.to_string(),
            questions,
            tool,
        };
        let mut state = self.state.lock().unwrap();
        state.pending.insert(
            id,
            PendingEntry {
                info: info.clone(),
                tx,
            },
        );
        // TODO(integration): publish Event.Asked through the oc-core event bus.
        (AskHandle(rx), info)
    }

    /// Resolve a pending question with the user's answers.
    pub fn reply(
        &self,
        request_id: &QuestionId,
        answers: Vec<Answer>,
    ) -> Result<(), QuestionError> {
        let mut state = self.state.lock().unwrap();
        let Some(entry) = state.pending.remove(request_id) else {
            tracing::warn!(request_id = %request_id, "reply for unknown request");
            return Err(QuestionError::NotFound {
                request_id: request_id.clone(),
            });
        };
        tracing::info!(request_id = %request_id, answers = answers.len(), "replied");
        // TODO(integration): publish Event.Replied through the oc-core event bus.
        let _ = entry.tx.send(Ok(answers));
        Ok(())
    }

    /// Dismiss a pending question.
    pub fn reject(&self, request_id: &QuestionId) -> Result<(), QuestionError> {
        let mut state = self.state.lock().unwrap();
        let Some(entry) = state.pending.remove(request_id) else {
            tracing::warn!(request_id = %request_id, "reject for unknown request");
            return Err(QuestionError::NotFound {
                request_id: request_id.clone(),
            });
        };
        tracing::info!(request_id = %request_id, "rejected");
        // TODO(integration): publish Event.Rejected through the oc-core event bus.
        let _ = entry.tx.send(Err(QuestionError::Rejected));
        Ok(())
    }

    /// All currently pending requests.
    pub fn list(&self) -> Vec<Request> {
        let state = self.state.lock().unwrap();
        state
            .pending
            .values()
            .map(|entry| entry.info.clone())
            .collect()
    }
}
