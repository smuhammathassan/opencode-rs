//! Typed rows and CRUD helpers for the schema tables.
//!
//! Mirrors the reference's Drizzle table objects (`packages/core/src/**/sql.ts`)
//! and the data access helpers that query them (`message-v2.ts`, `session.ts`).
//! Struct field names equal column names so rows round-trip through the JSON
//! mapping in [`crate::sqlite::Row`].

use serde::{Deserialize, Serialize};

use crate::database::Database;
use crate::error::Result;
use crate::sqlite::{Queryable, Value};

/// Columns persisted as JSON text for each table.
pub const JSON_COLUMNS: &[(&str, &[&str])] = &[
    ("workspace", &["extra"]),
    ("event", &["data"]),
    ("project", &["sandboxes", "commands"]),
    ("message", &["data"]),
    ("part", &["data"]),
    ("session_context_epoch", &["snapshot"]),
    ("session_input", &["prompt"]),
    ("session_message", &["data"]),
    (
        "session",
        &["summary_diffs", "metadata", "revert", "permission", "model"],
    ),
];

pub fn json_columns(table: &str) -> &'static [&'static str] {
    JSON_COLUMNS
        .iter()
        .find(|(name, _)| *name == table)
        .map(|(_, columns)| *columns)
        .unwrap_or(&[])
}

macro_rules! rows {
    ($( $name:ident { $( $field:ident : $ty:ty ),* $(,)? } ),* $(,)?) => {
        $(
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
            pub struct $name {
                $(pub $field: $ty,)*
            }
        )*
    };
}

rows! {
    ProjectRow {
        id: String,
        worktree: String,
        vcs: Option<String>,
        name: Option<String>,
        icon_url: Option<String>,
        icon_url_override: Option<String>,
        icon_color: Option<String>,
        time_created: i64,
        time_updated: i64,
        time_initialized: Option<i64>,
        sandboxes: serde_json::Value,
        commands: Option<serde_json::Value>,
    },
    WorkspaceRow {
        id: String,
        r#type: String,
        name: String,
        branch: Option<String>,
        directory: Option<String>,
        extra: Option<serde_json::Value>,
        project_id: String,
        time_used: i64,
    },
    SessionRow {
        id: String,
        project_id: String,
        workspace_id: Option<String>,
        parent_id: Option<String>,
        slug: String,
        directory: String,
        path: Option<String>,
        title: String,
        version: String,
        share_url: Option<String>,
        summary_additions: Option<i64>,
        summary_deletions: Option<i64>,
        summary_files: Option<i64>,
        summary_diffs: Option<serde_json::Value>,
        metadata: Option<serde_json::Value>,
        cost: f64,
        tokens_input: i64,
        tokens_output: i64,
        tokens_reasoning: i64,
        tokens_cache_read: i64,
        tokens_cache_write: i64,
        revert: Option<serde_json::Value>,
        permission: Option<serde_json::Value>,
        agent: Option<String>,
        model: Option<serde_json::Value>,
        time_created: i64,
        time_updated: i64,
        time_compacting: Option<i64>,
        time_archived: Option<i64>,
    },
    MessageRow {
        id: String,
        session_id: String,
        time_created: i64,
        time_updated: i64,
        data: serde_json::Value,
    },
    PartRow {
        id: String,
        message_id: String,
        session_id: String,
        time_created: i64,
        time_updated: i64,
        data: serde_json::Value,
    },
    TodoRow {
        session_id: String,
        content: String,
        status: String,
        priority: String,
        position: i64,
        time_created: i64,
        time_updated: i64,
    },
    SessionMessageRow {
        id: String,
        session_id: String,
        r#type: String,
        seq: i64,
        time_created: i64,
        time_updated: i64,
        data: serde_json::Value,
    },
    SessionInputRow {
        id: String,
        session_id: String,
        prompt: serde_json::Value,
        delivery: String,
        admitted_seq: i64,
        promoted_seq: Option<i64>,
        time_created: i64,
    },
    SessionContextEpochRow {
        session_id: String,
        baseline: String,
        snapshot: serde_json::Value,
        baseline_seq: i64,
    },
    EventSequenceRow {
        aggregate_id: String,
        seq: i64,
        owner_id: Option<String>,
    },
    EventRow {
        id: String,
        aggregate_id: String,
        seq: i64,
        r#type: String,
        data: serde_json::Value,
    },
    CredentialRow {
        id: String,
        integration_id: Option<String>,
        label: String,
        value: String,
        connector_id: Option<String>,
        method_id: Option<String>,
        active: Option<i64>,
        time_created: i64,
        time_updated: i64,
    },
    PermissionRow {
        id: String,
        project_id: String,
        action: String,
        resource: String,
        time_created: i64,
        time_updated: i64,
    },
    AccountRow {
        id: String,
        email: String,
        url: String,
        access_token: String,
        refresh_token: String,
        token_expiry: Option<i64>,
        time_created: i64,
        time_updated: i64,
    },
    ControlAccountRow {
        email: String,
        url: String,
        access_token: String,
        refresh_token: String,
        token_expiry: Option<i64>,
        active: i64,
        time_created: i64,
        time_updated: i64,
    },
    AccountStateRow {
        id: i64,
        active_account_id: Option<String>,
        active_org_id: Option<String>,
    },
    DataMigrationRow {
        name: String,
        time_completed: i64,
    },
    ProjectDirectoryRow {
        project_id: String,
        directory: String,
        r#type: Option<String>,
        strategy: Option<String>,
        time_created: i64,
    },
    SessionShareRow {
        session_id: String,
        id: String,
        secret: String,
        url: String,
        time_created: i64,
        time_updated: i64,
    }
}

impl Database {
    /// Fetch a persisted project by its stable project id.
    pub fn get_project(&self, project_id: &str) -> Result<Option<ProjectRow>> {
        self.get_by(
            "project",
            "id",
            &Value::Text(project_id.to_string()),
            json_columns("project"),
        )
    }

    /// Fetch the project whose canonical worktree matches `worktree`.
    pub fn get_project_by_worktree(&self, worktree: &str) -> Result<Option<ProjectRow>> {
        self.get_by(
            "project",
            "worktree",
            &Value::Text(worktree.to_string()),
            json_columns("project"),
        )
    }

    /// List all persisted project rows.
    pub fn list_projects(&self) -> Result<Vec<ProjectRow>> {
        self.list("project", json_columns("project"))
    }

    /// Persist a project row using the same idempotent upsert semantics as the
    /// reference Project service's `onConflictDoUpdate` path.
    pub fn upsert_project(&self, row: &ProjectRow) -> Result<()> {
        self.upsert(
            "project",
            row,
            json_columns("project"),
            "id",
            &Value::Text(row.id.clone()),
        )
    }

    /// `MessageV2.get` shape — message by `id` scoped to `session_id`.
    /// From reference/packages/opencode/src/session/message-v2.ts:506
    pub fn get_message(&self, message_id: &str, session_id: &str) -> Result<Option<MessageRow>> {
        let sql = "SELECT * FROM `message` WHERE `id` = ? AND `session_id` = ? LIMIT 1";
        match self.db.get(
            sql,
            &[
                Value::Text(message_id.into()),
                Value::Text(session_id.into()),
            ],
        )? {
            Some(row) => Ok(Some(row.from_row(json_columns("message"))?)),
            None => Ok(None),
        }
    }

    /// `MessageV2.parts` — parts of a message ordered by id.
    /// From reference/packages/opencode/src/session/message-v2.ts:492
    pub fn list_parts(&self, message_id: &str) -> Result<Vec<PartRow>> {
        let sql = "SELECT * FROM `part` WHERE `message_id` = ? ORDER BY `id`";
        self.db
            .run_all(sql, &[Value::Text(message_id.into())])?
            .iter()
            .map(|row| row.from_row(json_columns("part")))
            .collect()
    }

    /// `MessageV2.hydrate` — parts for many message ids ordered by message id,
    /// then id. From reference/packages/opencode/src/session/message-v2.ts:98
    pub fn list_parts_by_messages(&self, message_ids: &[&str]) -> Result<Vec<PartRow>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = vec!["?"; message_ids.len()].join(", ");
        let sql = format!("SELECT * FROM `part` WHERE `message_id` IN ({placeholders}) ORDER BY `message_id`, `id`");
        let params: Vec<Value> = message_ids
            .iter()
            .map(|id| Value::Text(id.to_string()))
            .collect();
        self.db
            .run_all(&sql, &params)?
            .iter()
            .map(|row| row.from_row(json_columns("part")))
            .collect()
    }

    /// `MessageV2.page` — newest messages of a session, newest first, with a
    /// `(time, id)` cursor, returning `limit + 1` rows so the caller can detect
    /// another page. From reference/packages/opencode/src/session/message-v2.ts:425
    pub fn list_messages_page(
        &self,
        session_id: &str,
        limit: i64,
        before: Option<(&str, i64)>,
    ) -> Result<Vec<MessageRow>> {
        let sql = match before {
            Some(_) => {
                "SELECT * FROM `message` WHERE `session_id` = ? AND (`time_created` < ? OR (`time_created` = ? AND `id` < ?)) ORDER BY `time_created` DESC, `id` DESC LIMIT ?"
            }
            None => "SELECT * FROM `message` WHERE `session_id` = ? ORDER BY `time_created` DESC, `id` DESC LIMIT ?",
        };
        let params: Vec<Value> = match before {
            Some((id, time)) => vec![
                Value::Text(session_id.into()),
                Value::Integer(time),
                Value::Integer(time),
                Value::Text(id.to_string()),
                Value::Integer(limit + 1),
            ],
            None => vec![Value::Text(session_id.into()), Value::Integer(limit + 1)],
        };
        self.db
            .run_all(sql, &params)?
            .iter()
            .map(|row| row.from_row(json_columns("message")))
            .collect()
    }

    /// Load the event-sourced session message stream in sequence order.
    ///
    /// The session runner consumes this table rather than the legacy
    /// `message` projection when it reconstructs V2 history.
    pub fn list_session_messages(&self, session_id: &str) -> Result<Vec<SessionMessageRow>> {
        let sql = "SELECT * FROM `session_message` WHERE `session_id` = ? ORDER BY `seq`";
        self.db
            .run_all(sql, &[Value::Text(session_id.into())])?
            .iter()
            .map(|row| row.from_row(json_columns("session_message")))
            .collect()
    }

    /// Load one event-sourced session message by its stable id.
    pub fn get_session_message(&self, message_id: &str) -> Result<Option<SessionMessageRow>> {
        self.get_by(
            "session_message",
            "id",
            &Value::Text(message_id.into()),
            json_columns("session_message"),
        )
    }

    /// Return the newest compaction sequence for a session, if one exists.
    pub fn latest_compaction_seq(&self, session_id: &str) -> Result<Option<i64>> {
        let sql = "SELECT `seq` FROM `session_message` WHERE `session_id` = ? AND `type` = 'compaction' ORDER BY `seq` DESC LIMIT 1";
        self.db
            .get(sql, &[Value::Text(session_id.into())])?
            .map(|row| row.get_by_name::<i64>("seq"))
            .transpose()
    }

    /// Load the context-epoch baseline used to trim runner history.
    pub fn context_epoch(&self, session_id: &str) -> Result<Option<SessionContextEpochRow>> {
        self.get_by(
            "session_context_epoch",
            "session_id",
            &Value::Text(session_id.into()),
            json_columns("session_context_epoch"),
        )
    }

    /// Whether a session exists.
    pub fn session_exists(&self, session_id: &str) -> Result<bool> {
        let sql = "SELECT `id` FROM `session` WHERE `id` = ? LIMIT 1";
        Ok(self
            .db
            .get(sql, &[Value::Text(session_id.into())])?
            .is_some())
    }

    /// Fetch a session by id.
    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionRow>> {
        self.get_by(
            "session",
            "id",
            &Value::Text(session_id.into()),
            json_columns("session"),
        )
    }

    /// List sessions, newest first. `include_archived` mirrors the reference's
    /// `isNull(time_archived)` filter.
    /// From reference/packages/opencode/src/session/session.ts:556
    pub fn list_sessions(&self, include_archived: bool) -> Result<Vec<SessionRow>> {
        let sql = if include_archived {
            "SELECT * FROM `session` ORDER BY `time_updated` DESC, `id` DESC"
        } else {
            "SELECT * FROM `session` WHERE `time_archived` IS NULL ORDER BY `time_updated` DESC, `id` DESC"
        };
        self.db
            .run(sql)?
            .iter()
            .map(|row| row.from_row(json_columns("session")))
            .collect()
    }

    /// List todos of a session.
    /// From reference/packages/opencode/src/session/todo.ts
    pub fn list_todos(&self, session_id: &str) -> Result<Vec<TodoRow>> {
        let sql = "SELECT * FROM `todo` WHERE `session_id` = ? ORDER BY `position`";
        self.db
            .run_all(sql, &[Value::Text(session_id.into())])?
            .iter()
            .map(|row| row.from_row(json_columns("todo")))
            .collect()
    }

    /// Load the durable sync cursors used by the event store.
    pub fn list_event_sequences(&self) -> Result<Vec<EventSequenceRow>> {
        self.list("event_sequence", &[])
    }

    /// Load durable sync events in aggregate/cursor order for store hydration.
    pub fn list_events(&self) -> Result<Vec<EventRow>> {
        self.db
            .run("SELECT * FROM `event` ORDER BY `aggregate_id`, `seq`")?
            .iter()
            .map(|row| row.from_row(json_columns("event")))
            .collect()
    }

    /// Append a sequence cursor and its event row as one SQLite transaction.
    /// The expected cursor check keeps multiple store handles from silently
    /// overwriting one another's event order.
    pub fn persist_event(&self, sequence: &EventSequenceRow, event: &EventRow) -> Result<()> {
        if sequence.aggregate_id != event.aggregate_id || sequence.seq != event.seq {
            return Err(crate::error::Error::Row(
                "event sequence and event row disagree".into(),
            ));
        }
        let data = serde_json::to_string(&event.data)?;
        self.db.transaction(|tx| {
            let params = [Value::Text(sequence.aggregate_id.clone())];
            let current = tx.run_get(
                "SELECT `seq`, `owner_id` FROM `event_sequence` WHERE `aggregate_id` = ?",
                &params,
            )?;
            match current {
                Some(row) => {
                    let current_seq = row.get_by_name::<i64>("seq")?;
                    let current_owner = row.get_by_name::<Option<String>>("owner_id")?;
                    if current_seq + 1 != sequence.seq {
                        return Err(crate::error::Error::Row(format!(
                            "event sequence mismatch for {}: expected {}, got {}",
                            sequence.aggregate_id,
                            current_seq + 1,
                            sequence.seq
                        )));
                    }
                    if current_owner != sequence.owner_id {
                        return Err(crate::error::Error::Row(format!(
                            "event owner mismatch for {}",
                            sequence.aggregate_id
                        )));
                    }
                    tx.run_exec(
                        "UPDATE `event_sequence` SET `seq` = ? WHERE `aggregate_id` = ?",
                        &[
                            Value::Integer(sequence.seq),
                            Value::Text(sequence.aggregate_id.clone()),
                        ],
                    )?;
                }
                None => {
                    if sequence.seq != 0 {
                        return Err(crate::error::Error::Row(format!(
                            "event sequence mismatch for {}: expected 0, got {}",
                            sequence.aggregate_id, sequence.seq
                        )));
                    }
                    tx.run_exec(
                        "INSERT INTO `event_sequence` (`aggregate_id`, `seq`, `owner_id`) VALUES (?, ?, ?)",
                        &[
                            Value::Text(sequence.aggregate_id.clone()),
                            Value::Integer(sequence.seq),
                            sequence
                                .owner_id
                                .clone()
                                .map_or(Value::Null, Value::Text),
                        ],
                    )?;
                }
            }
            tx.run_exec(
                "INSERT INTO `event` (`id`, `aggregate_id`, `seq`, `type`, `data`) VALUES (?, ?, ?, ?, ?)",
                &[
                    Value::Text(event.id.clone()),
                    Value::Text(event.aggregate_id.clone()),
                    Value::Integer(event.seq),
                    Value::Text(event.r#type.clone()),
                    Value::Text(data),
                ],
            )?;
            Ok(())
        })
    }

    /// Persist the owner claim for an aggregate.
    pub fn claim_event_owner(&self, aggregate_id: &str, owner_id: &str) -> Result<()> {
        self.update_by(
            "event_sequence",
            "owner_id",
            &Value::Text(owner_id.to_string()),
            "aggregate_id",
            &Value::Text(aggregate_id.to_string()),
        )?;
        Ok(())
    }

    /// Remove an aggregate's cursor; its events cascade through the schema FK.
    pub fn remove_event_aggregate(&self, aggregate_id: &str) -> Result<()> {
        self.delete_by(
            "event_sequence",
            "aggregate_id",
            &Value::Text(aggregate_id.to_string()),
        )?;
        Ok(())
    }

    /// Append an event for an aggregate. Returns the new sequence number.
    pub fn append_event(&self, aggregate_id: &str, owner_id: Option<&str>) -> Result<i64> {
        let sql = "INSERT INTO `event_sequence` (`aggregate_id`, `seq`, `owner_id`) \
                   VALUES (?, 0, ?) \
                   ON CONFLICT(`aggregate_id`) DO UPDATE SET `seq` = `event_sequence`.`seq` + 1 \
                   RETURNING `seq`";
        let params = vec![
            Value::Text(aggregate_id.into()),
            owner_id.map_or(Value::Null, |owner| Value::Text(owner.to_string())),
        ];
        let row = self.db.get(sql, &params)?.ok_or_else(|| {
            crate::error::Error::Row("event sequence upsert returned no row".into())
        })?;
        row.get_by_name::<i64>("seq")
    }
}
