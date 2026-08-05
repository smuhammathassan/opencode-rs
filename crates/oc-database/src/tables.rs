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
