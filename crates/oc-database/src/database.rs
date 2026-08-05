//! The `Database` service.
//!
//! Port of `reference/packages/core/src/database/database.ts`. Opens the
//! SQLite connection, applies the PRAGMA battery and runs migrations, and
//! resolves the database file path exactly like `Database.path()`.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::sqlite::{Config, Queryable, Sqlite, Value};

/// `Database.path()` — resolve the database file.
/// From reference/packages/core/src/database/database.ts:43
pub fn path() -> PathBuf {
    let data = data_dir();
    if let Some(db) = std::env::var("OPENCODE_DB")
        .ok()
        .filter(|value| !value.is_empty())
    {
        if db == ":memory:" || Path::new(&db).is_absolute() {
            return PathBuf::from(db);
        }
        return data.join(db);
    }
    let channel = std::env::var("OPENCODE_CHANNEL").unwrap_or_else(|_| "local".to_string());
    let disable_channel_db = std::env::var("OPENCODE_DISABLE_CHANNEL_DB")
        .map(|value| value == "1" || value == "true")
        .unwrap_or(false);
    if ["latest", "beta", "prod"].contains(&channel.as_str()) || disable_channel_db {
        data.join("opencode.db")
    } else {
        let sanitized: String = channel
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        data.join(format!("opencode-{sanitized}.db"))
    }
}

/// XDG data dir for the `opencode` app (`~/.local/share/opencode`).
/// From reference/packages/core/src/global.ts:9
fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "opencode")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("opencode"))
}

/// The database service. `db` is the underlying [`Sqlite`] client.
/// From reference/packages/core/src/database/database.ts:16
pub struct Database {
    pub db: Sqlite,
}

impl Database {
    /// Open the database at `filename`, applying the reference's PRAGMA setup
    /// and running migrations. Initialization (including the WAL `PRAGMA`,
    /// which needs an exclusive lock) is serialized process-wide so concurrent
    /// opens of one database path are safe.
    /// From reference/packages/core/src/database/database.ts:22
    pub fn open(filename: impl AsRef<Path>) -> Result<Self> {
        let _guard = crate::migration::apply_lock().map_err(|_| Error::Poisoned)?;
        let filename = filename.as_ref();
        if let Some(parent) = filename.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let db = Sqlite::open(Config::layer(filename.to_string_lossy().into_owned()))?;
        // PRAGMAs may return rows (e.g. `journal_mode` on an in-memory db), so
        // run them through `execute_batch` rather than `execute`.
        // From reference/packages/core/src/database/database.ts:27
        db.run_batch("PRAGMA journal_mode = WAL")?;
        db.run_batch("PRAGMA synchronous = NORMAL")?;
        db.run_batch("PRAGMA busy_timeout = 5000")?;
        db.run_batch("PRAGMA cache_size = -64000")?;
        db.run_batch("PRAGMA foreign_keys = ON")?;
        db.run_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        crate::migration::apply_inner(&db)?;
        Ok(Self { db })
    }

    /// Open an in-memory database (migrations applied). Used by tests and the
    /// `:memory:` `OPENCODE_DB` path.
    pub fn open_memory() -> Result<Self> {
        Self::open(":memory:")
    }

    /// Typed insert: serializes `row` (struct field names must equal column
    /// names) into an `INSERT INTO \`table\`` statement. `json_columns` lists
    /// columns persisted as JSON text.
    pub fn insert<T: serde::Serialize>(
        &self,
        table: &str,
        row: &T,
        json_columns: &[&str],
    ) -> Result<()> {
        let object = serde_json::to_value(row)?
            .as_object()
            .cloned()
            .ok_or_else(|| Error::Row(format!("row for {table} must be a JSON object")))?;
        if object.is_empty() {
            return Err(Error::Row(format!("row for {table} has no columns")));
        }
        let columns: Vec<String> = object.keys().cloned().collect();
        let values: Result<Vec<Value>> = object
            .values()
            .zip(columns.iter())
            .map(|(value, column)| {
                super::sqlite::json_to_sqlite(value, json_columns.contains(&column.as_str()))
            })
            .collect();
        let placeholders = vec!["?"; columns.len()].join(", ");
        let quoted: Vec<String> = columns.iter().map(|c| format!("`{c}`")).collect();
        let sql = format!(
            "INSERT INTO `{table}` ({}) VALUES ({placeholders})",
            quoted.join(", ")
        );
        self.db.execute_with(&sql, &values?)?;
        Ok(())
    }

    /// Fetch all rows of `table`, deserialized into `T`.
    pub fn list<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        json_columns: &[&str],
    ) -> Result<Vec<T>> {
        let sql = format!("SELECT * FROM `{table}`");
        self.db
            .run(&sql)?
            .iter()
            .map(|row| row.from_row::<T>(json_columns))
            .collect()
    }

    /// Fetch a single row by equality on `column`.
    pub fn get_by<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        column: &str,
        value: &Value,
        json_columns: &[&str],
    ) -> Result<Option<T>> {
        let sql = format!("SELECT * FROM `{table}` WHERE `{column}` = ? LIMIT 1");
        match self.db.get(&sql, std::slice::from_ref(value))? {
            Some(row) => Ok(Some(row.from_row::<T>(json_columns)?)),
            None => Ok(None),
        }
    }

    /// Delete rows matching `WHERE column = ?`.
    pub fn delete_by(&self, table: &str, column: &str, value: &Value) -> Result<usize> {
        let sql = format!("DELETE FROM `{table}` WHERE `{column}` = ?");
        self.db.execute_with(&sql, std::slice::from_ref(value))
    }

    /// Delete rows matching `WHERE column IN (...)`.
    pub fn delete_by_in(&self, table: &str, column: &str, values: &[Value]) -> Result<usize> {
        if values.is_empty() {
            return Ok(0);
        }
        let placeholders = vec!["?"; values.len()].join(", ");
        let sql = format!("DELETE FROM `{table}` WHERE `{column}` IN ({placeholders})");
        self.db.execute_with(&sql, values)
    }

    /// Update `column = value` for rows matching `WHERE key = key_value`.
    pub fn update_by(
        &self,
        table: &str,
        column: &str,
        value: &Value,
        key: &str,
        key_value: &Value,
    ) -> Result<usize> {
        let sql = format!("UPDATE `{table}` SET `{column}` = ? WHERE `{key}` = ?");
        self.db
            .execute_with(&sql, &[value.clone(), key_value.clone()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_resolution() {
        std::env::remove_var("OPENCODE_DB");
        std::env::remove_var("OPENCODE_CHANNEL");
        std::env::remove_var("OPENCODE_DISABLE_CHANNEL_DB");
        let data = data_dir();
        assert_eq!(path(), data.join("opencode-local.db"));
        std::env::set_var("OPENCODE_CHANNEL", "beta");
        assert_eq!(path(), data.join("opencode.db"));
        std::env::set_var("OPENCODE_CHANNEL", "canary");
        assert_eq!(path(), data.join("opencode-canary.db"));
        std::env::set_var("OPENCODE_CHANNEL", "local");
        std::env::set_var("OPENCODE_DB", ":memory:");
        assert_eq!(path(), PathBuf::from(":memory:"));
        std::env::set_var("OPENCODE_DB", "/custom/db.sqlite");
        assert_eq!(path(), PathBuf::from("/custom/db.sqlite"));
        std::env::set_var("OPENCODE_DB", "relative.db");
        assert_eq!(path(), data.join("relative.db"));
        std::env::remove_var("OPENCODE_DB");
    }

    #[test]
    fn open_and_crud() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct Project {
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
        }

        let db = Database::open_memory().unwrap();
        let project = Project {
            id: "global".to_string(),
            worktree: "/".to_string(),
            vcs: None,
            name: Some("global".to_string()),
            icon_url: None,
            icon_url_override: None,
            icon_color: None,
            time_created: 1,
            time_updated: 1,
            time_initialized: None,
            sandboxes: serde_json::json!([]),
            commands: None,
        };
        db.insert("project", &project, &["sandboxes", "commands"])
            .unwrap();
        let fetched: Project = db
            .get_by(
                "project",
                "id",
                &Value::Text("global".into()),
                &["sandboxes", "commands"],
            )
            .unwrap()
            .unwrap();
        assert_eq!(fetched, project);
        assert_eq!(
            db.list::<Project>("project", &["sandboxes", "commands"])
                .unwrap()
                .len(),
            1
        );
        assert!(db
            .get_by::<Project>(
                "project",
                "id",
                &Value::Text("nope".into()),
                &["sandboxes", "commands"]
            )
            .unwrap()
            .is_none());
        assert_eq!(
            db.update_by(
                "project",
                "name",
                &Value::Text("renamed".into()),
                "id",
                &Value::Text("global".into())
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.delete_by("project", "id", &Value::Text("global".into()))
                .unwrap(),
            1
        );
        assert!(db
            .list::<Project>("project", &["sandboxes", "commands"])
            .unwrap()
            .is_empty());
    }
}
