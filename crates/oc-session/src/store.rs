/// From reference/packages/core/src/session/store.ts
///
/// The session store abstraction the runner/server implement. Mirrors the
/// `SessionStore.Interface` service.
///
/// TODO(integration): implement against oc-database once the schema lands.
use crate::history::MessageRow;
use crate::v1::SessionInfo;
use crate::v2::Message;

pub trait SessionDb {
    /// `SessionContextEpochTable.baseline_seq` for the session, if any.
    fn context_epoch_baseline(&self, session_id: &str) -> Option<u64>;
    /// The seq of the latest compaction message for the session, if any.
    fn latest_compaction_seq(&self, session_id: &str) -> Option<u64>;
    /// All `SessionMessageTable` rows for the session.
    fn message_rows(&self, session_id: &str) -> Vec<MessageRow>;
    /// A single message row by id, if any.
    fn message_row(&self, message_id: &str) -> Option<MessageRow>;
    /// `SessionTable` row for the session, if any.
    fn session_row(&self, session_id: &str) -> Option<SessionInfo>;
}

pub trait SessionStore {
    /// `SessionStore.get`.
    fn get(&self, session_id: &str) -> Option<SessionInfo>;
    /// `SessionStore.context` — full decoded history.
    fn context(&self, session_id: &str)
        -> Vec<Result<Message, crate::history::MessageDecodeError>>;
    /// `SessionStore.runnerContext` — history for the runner at a baseline.
    fn runner_context(
        &self,
        session_id: &str,
        baseline_seq: u64,
    ) -> Vec<Result<Message, crate::history::MessageDecodeError>>;
    /// `SessionStore.message` — single message by id.
    fn message(
        &self,
        message_id: &str,
    ) -> Option<(String, Result<Message, crate::history::MessageDecodeError>)>;
}

/// A [`SessionStore`] backed by a [`SessionDb`].
#[derive(Debug, Clone, Copy)]
pub struct DbSessionStore<'a, D: SessionDb> {
    pub db: &'a D,
}

impl<'a, D: SessionDb> SessionStore for DbSessionStore<'a, D> {
    fn get(&self, session_id: &str) -> Option<SessionInfo> {
        self.db.session_row(session_id)
    }

    fn context(
        &self,
        session_id: &str,
    ) -> Vec<Result<Message, crate::history::MessageDecodeError>> {
        crate::history::load(self.db, session_id)
    }

    fn runner_context(
        &self,
        session_id: &str,
        baseline_seq: u64,
    ) -> Vec<Result<Message, crate::history::MessageDecodeError>> {
        crate::history::load_for_runner(self.db, session_id, baseline_seq)
    }

    fn message(
        &self,
        message_id: &str,
    ) -> Option<(String, Result<Message, crate::history::MessageDecodeError>)> {
        let row = self.db.message_row(message_id)?;
        Some((row.session_id.clone(), crate::history::decode_row(row)))
    }
}
