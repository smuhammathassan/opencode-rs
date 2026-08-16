/// From reference/packages/core/src/session/store.ts
///
/// The session store abstraction the runner/server implement. Mirrors the
/// `SessionStore.Interface` service.
///
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

/// SQLite-backed implementation of [`SessionDb`].
///
/// This adapter keeps the session orchestration crate independent from SQL
/// details while allowing the production runner to consume the durable
/// `session`, `session_message`, and `session_context_epoch` projections.
#[derive(Clone, Copy)]
pub struct SqliteSessionDb<'a> {
    pub database: &'a oc_database::Database,
}

impl<'a> SqliteSessionDb<'a> {
    pub fn new(database: &'a oc_database::Database) -> Self {
        Self { database }
    }
}

fn session_info_from_row(row: oc_database::tables::SessionRow) -> SessionInfo {
    let summary = match (
        row.summary_additions,
        row.summary_deletions,
        row.summary_files,
    ) {
        (Some(additions), Some(deletions), Some(files)) => Some(crate::v1::SessionSummary {
            additions: additions as f64,
            deletions: deletions as f64,
            files: files as f64,
            diffs: row
                .summary_diffs
                .and_then(|value| serde_json::from_value(value).ok()),
        }),
        _ => None,
    };
    let metadata = row
        .metadata
        .and_then(|value| serde_json::from_value(value).ok());
    let permission = row
        .permission
        .and_then(|value| serde_json::from_value(value).ok());
    let share = row.share_url.map(|url| crate::v1::SessionShare { url });
    let model = row
        .model
        .and_then(|value| serde_json::from_value(value).ok());
    SessionInfo {
        id: row.id,
        slug: row.slug,
        project_id: row.project_id,
        workspace_id: row.workspace_id,
        directory: row.directory,
        path: row.path,
        parent_id: row.parent_id,
        summary,
        cost: Some(row.cost),
        tokens: Some(crate::v1::SessionTokens {
            input: row.tokens_input as f64,
            output: row.tokens_output as f64,
            reasoning: row.tokens_reasoning as f64,
            cache: crate::v1::CacheTokens {
                read: row.tokens_cache_read as f64,
                write: row.tokens_cache_write as f64,
            },
        }),
        share,
        title: row.title,
        agent: row.agent,
        model,
        version: row.version,
        metadata,
        time: crate::v1::SessionTime {
            created: row.time_created.max(0) as u64,
            updated: row.time_updated.max(0) as u64,
            compacting: row.time_compacting.map(|value| value.max(0) as u64),
            archived: row.time_archived.map(|value| value as f64),
        },
        permission,
        revert: row
            .revert
            .and_then(|value| serde_json::from_value(value).ok()),
    }
}

impl SessionDb for SqliteSessionDb<'_> {
    fn context_epoch_baseline(&self, session_id: &str) -> Option<u64> {
        self.database
            .context_epoch(session_id)
            .ok()
            .flatten()
            .map(|row| row.baseline_seq.max(0) as u64)
    }

    fn latest_compaction_seq(&self, session_id: &str) -> Option<u64> {
        self.database
            .latest_compaction_seq(session_id)
            .ok()
            .flatten()
            .map(|seq| seq.max(0) as u64)
    }

    fn message_rows(&self, session_id: &str) -> Vec<MessageRow> {
        self.database
            .list_session_messages(session_id)
            .unwrap_or_default()
            .into_iter()
            .map(|row| MessageRow {
                seq: row.seq.max(0) as u64,
                id: row.id,
                session_id: row.session_id,
                type_: row.r#type,
                data: row.data,
            })
            .collect()
    }

    fn message_row(&self, message_id: &str) -> Option<MessageRow> {
        self.database
            .get_session_message(message_id)
            .ok()
            .flatten()
            .map(|row| MessageRow {
                seq: row.seq.max(0) as u64,
                id: row.id,
                session_id: row.session_id,
                type_: row.r#type,
                data: row.data,
            })
    }

    fn session_row(&self, session_id: &str) -> Option<SessionInfo> {
        self.database
            .get_session(session_id)
            .ok()
            .flatten()
            .map(session_info_from_row)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use oc_database::tables::{ProjectRow, SessionMessageRow, SessionRow};

    #[test]
    fn sqlite_adapter_loads_session_and_event_history() {
        let database = oc_database::Database::open_memory().unwrap();
        let columns = oc_database::tables::json_columns;
        database
            .insert(
                "project",
                &ProjectRow {
                    id: "global".into(),
                    worktree: "/work".into(),
                    vcs: None,
                    name: None,
                    icon_url: None,
                    icon_url_override: None,
                    icon_color: None,
                    time_created: 1,
                    time_updated: 1,
                    time_initialized: None,
                    sandboxes: serde_json::json!([]),
                    commands: None,
                },
                columns("project"),
            )
            .unwrap();
        database
            .insert(
                "session",
                &SessionRow {
                    id: "ses_1".into(),
                    project_id: "global".into(),
                    workspace_id: None,
                    parent_id: None,
                    slug: "first".into(),
                    directory: "/work".into(),
                    path: None,
                    title: "First".into(),
                    version: "v1".into(),
                    share_url: None,
                    summary_additions: None,
                    summary_deletions: None,
                    summary_files: None,
                    summary_diffs: None,
                    metadata: None,
                    cost: 0.0,
                    tokens_input: 0,
                    tokens_output: 0,
                    tokens_reasoning: 0,
                    tokens_cache_read: 0,
                    tokens_cache_write: 0,
                    revert: None,
                    permission: None,
                    agent: Some("build".into()),
                    model: None,
                    time_created: 1,
                    time_updated: 1,
                    time_compacting: None,
                    time_archived: None,
                },
                columns("session"),
            )
            .unwrap();
        database
            .insert(
                "session_message",
                &SessionMessageRow {
                    id: "msg_1".into(),
                    session_id: "ses_1".into(),
                    r#type: "user".into(),
                    seq: 1,
                    time_created: 1,
                    time_updated: 1,
                    data: serde_json::json!({
                        "role": "user",
                        "text": "hello",
                        "time": { "created": 1 }
                    }),
                },
                columns("session_message"),
            )
            .unwrap();

        let database_store = SqliteSessionDb::new(&database);
        let store = DbSessionStore {
            db: &database_store,
        };
        assert_eq!(store.get("ses_1").unwrap().title, "First");
        let history = store.context("ses_1");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].as_ref().unwrap().id(), "msg_1");
    }
}
