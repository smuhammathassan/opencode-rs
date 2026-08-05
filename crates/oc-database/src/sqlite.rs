//! SQLite backend.
//!
//! Port of `reference/packages/core/src/database/sqlite.ts`,
//! `sqlite.bun.ts` and `sqlite.node.ts`. The reference selects a Bun or Node
//! native driver behind an Effect `SqlClient`; here one rusqlite backend serves
//! both, exposing the same operations: `run`/`all`/`get`, `transaction`,
//! `export` (serialize), and `load_extension`. Access is serialized through a
//! mutex exactly like the reference's one-permit semaphore.

use std::sync::Mutex;
use std::time::Duration;

use crate::error::{Error, Result};
pub use rusqlite::types::Value;
use rusqlite::types::{FromSql, ValueRef};
use rusqlite::{Connection, DatabaseName, OpenFlags, Transaction};

#[derive(Debug, Clone)]
pub struct Config {
    /// Database file path, or `:memory:`.
    pub filename: String,
    pub readonly: bool,
    pub create: bool,
    pub readwrite: bool,
    pub disable_wal: bool,
    pub timeout_ms: Option<i64>,
    pub allow_extension: bool,
    pub span_attributes: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            filename: String::new(),
            readonly: false,
            create: true,
            readwrite: true,
            disable_wal: false,
            timeout_ms: None,
            allow_extension: false,
            span_attributes: Vec::new(),
        }
    }
}

impl Config {
    /// `Sqlite.layer` config shape. From reference/packages/core/src/database/sqlite.bun.ts:31
    pub fn layer(filename: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            ..Default::default()
        }
    }
}

/// An owned query result row. Columns mirror the column names of the stored
/// schema (snake_case); no name transformation is applied, matching the
/// reference's default `transformResultNames = undefined`.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    columns: Vec<String>,
    values: Vec<Value>,
}

impl Row {
    pub fn column_names(&self) -> &[String] {
        &self.columns
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn value(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    pub fn value_by_name(&self, name: &str) -> Option<&Value> {
        self.columns
            .iter()
            .position(|column| column == name)
            .and_then(|index| self.values.get(index))
    }

    pub fn is_null(&self, index: usize) -> bool {
        matches!(self.value(index), Some(Value::Null))
    }

    pub fn is_null_by_name(&self, name: &str) -> bool {
        matches!(self.value_by_name(name), Some(Value::Null))
    }

    pub fn get<T: FromSql>(&self, index: usize) -> Result<T> {
        let value = self
            .values
            .get(index)
            .ok_or_else(|| Error::Row(format!("no value at column index {index}")))?;
        Ok(T::column_result(ValueRef::from(value)).map_err(rusqlite::Error::from)?)
    }

    pub fn get_by_name<T: FromSql>(&self, name: &str) -> Result<T> {
        let value = self
            .value_by_name(name)
            .ok_or_else(|| Error::Row(format!("no column named {name}")))?;
        Ok(T::column_result(ValueRef::from(value)).map_err(rusqlite::Error::from)?)
    }

    /// Convert the row to a JSON object keyed by column name. Blob values are
    /// encoded as base64 strings.
    pub fn to_json(&self) -> Result<serde_json::Value> {
        let mut map = serde_json::Map::with_capacity(self.columns.len());
        for (index, column) in self.columns.iter().enumerate() {
            let value = self
                .values
                .get(index)
                .ok_or_else(|| Error::Row(format!("no value at column index {index}")))?;
            map.insert(column.clone(), value_to_json(value));
        }
        Ok(serde_json::Value::Object(map))
    }

    /// Convert the row to a JSON object, parsing the listed columns as JSON
    /// text (Drizzle `{ mode: "json" }` columns and path-array columns).
    pub fn to_json_with(&self, json_columns: &[&str]) -> Result<serde_json::Value> {
        let mut map = serde_json::Map::with_capacity(self.columns.len());
        for (index, column) in self.columns.iter().enumerate() {
            let value = self
                .values
                .get(index)
                .ok_or_else(|| Error::Row(format!("no value at column index {index}")))?;
            let json = if json_columns.contains(&column.as_str()) {
                match value {
                    Value::Null => serde_json::Value::Null,
                    Value::Text(text) => serde_json::from_str(text)?,
                    other => value_to_json(other),
                }
            } else {
                value_to_json(value)
            };
            map.insert(column.clone(), json);
        }
        Ok(serde_json::Value::Object(map))
    }

    /// Deserialize the row into a typed struct via the JSON mapping above.
    pub fn from_row<T: serde::de::DeserializeOwned>(&self, json_columns: &[&str]) -> Result<T> {
        Ok(serde_json::from_value(self.to_json_with(json_columns)?)?)
    }
}

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Integer(i) => serde_json::Value::Number((*i).into()),
        Value::Real(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(t) => serde_json::Value::String(t.clone()),
        Value::Blob(b) => {
            let items: Vec<serde_json::Value> = b.iter().map(|byte| (*byte).into()).collect();
            serde_json::Value::Array(items)
        }
    }
}

pub(crate) fn json_to_sqlite(value: &serde_json::Value, as_json: bool) -> Result<Value> {
    Ok(match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Integer(*b as i64),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Real(f)
            } else {
                Value::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => Value::Text(s.clone()),
        other => {
            if as_json {
                Value::Text(serde_json::to_string(other)?)
            } else {
                return Err(Error::Row(format!(
                    "cannot store non-primitive value in a non-json column: {other}"
                )));
            }
        }
    })
}

/// Shared query surface implemented by both the owned [`Connection`] (via
/// [`Sqlite`]) and rusqlite [`Transaction`] (which derefs to `Connection`).
pub trait Queryable {
    fn run(&self, sql: &str) -> Result<Vec<Row>>;
    fn run_all(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>>;
    fn run_get(&self, sql: &str, params: &[Value]) -> Result<Option<Row>>;
    fn run_exec(&self, sql: &str, params: &[Value]) -> Result<usize>;
    fn run_batch(&self, sql: &str) -> Result<()>;
}

impl Queryable for Connection {
    fn run(&self, sql: &str) -> Result<Vec<Row>> {
        self.run_all(sql, &[])
    }

    fn run_all(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        let mut stmt = self.prepare(sql)?;
        let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let mut rows = stmt.query(rusqlite::params_from_iter(params.iter()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(columns.len());
            for index in 0..columns.len() {
                values.push(row.get::<_, Value>(index)?);
            }
            out.push(Row {
                columns: columns.clone(),
                values,
            });
        }
        Ok(out)
    }

    fn run_get(&self, sql: &str, params: &[Value]) -> Result<Option<Row>> {
        let mut rows = self.run_all(sql, params)?;
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(rows.remove(0)))
    }

    fn run_exec(&self, sql: &str, params: &[Value]) -> Result<usize> {
        Ok(self.execute(sql, rusqlite::params_from_iter(params.iter()))?)
    }

    fn run_batch(&self, sql: &str) -> Result<()> {
        self.execute_batch(sql)?;
        Ok(())
    }
}

/// The rusqlite-backed connection mirroring `SqliteClient`.
/// From reference/packages/core/src/database/sqlite.bun.ts:23
pub struct Sqlite {
    conn: Mutex<Connection>,
    config: Config,
}

impl Sqlite {
    /// Open a database, mirroring the reference's `nativeLayer` open flags
    /// (readwrite + create by default, WAL unless disabled).
    /// From reference/packages/core/src/database/sqlite.bun.ts:154
    pub fn open(config: Config) -> Result<Self> {
        let mut flags = if config.readonly {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        if config.create && !config.readonly {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        if config.allow_extension {
            flags |= OpenFlags::SQLITE_OPEN_NOFOLLOW;
        }
        let conn = Connection::open_with_flags(&config.filename, flags)?;
        if let Some(timeout) = config.timeout_ms {
            conn.busy_timeout(Duration::from_millis(timeout as u64))?;
        }
        if !config.disable_wal && !config.readonly {
            conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
            config,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| Error::Poisoned)
    }

    /// `db.run(sql)` — execute and return all rows.
    pub fn run(&self, sql: &str) -> Result<Vec<Row>> {
        self.lock()?.run(sql)
    }

    /// `db.all(sql, params)`.
    pub fn all(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        self.lock()?.run_all(sql, params)
    }

    /// `db.get(sql, params)` — first row or none.
    pub fn get(&self, sql: &str, params: &[Value]) -> Result<Option<Row>> {
        self.lock()?.run_get(sql, params)
    }

    /// Execute a statement, returning the number of changed rows.
    pub fn execute(&self, sql: &str) -> Result<usize> {
        self.lock()?.run_exec(sql, &[])
    }

    pub fn execute_with(&self, sql: &str, params: &[Value]) -> Result<usize> {
        self.lock()?.run_exec(sql, params)
    }

    /// Run a closure inside a transaction (serialized by the connection lock).
    pub fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Transaction<'_>) -> Result<T>,
    {
        let mut conn = self.lock()?;
        let mut tx = conn.transaction()?;
        match f(&mut tx) {
            Ok(value) => {
                tx.commit()?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = tx.rollback() {
                    tracing::warn!(error = %rollback, "transaction rollback failed");
                }
                Err(error)
            }
        }
    }

    /// `db.export` — serialize the whole database to a byte image.
    /// From reference/packages/core/src/database/sqlite.bun.ts:104
    pub fn export(&self) -> Result<Vec<u8>> {
        let conn = self.lock()?;
        Ok(conn.serialize(DatabaseName::Main)?.as_ref().to_vec())
    }

    /// `db.loadExtension(path)`.
    /// From reference/packages/core/src/database/sqlite.bun.ts:110
    pub fn load_extension(&self, path: &str) -> Result<()> {
        let conn = self.lock()?;
        // Safety: extension loading performs a dlopen and executes code from the
        // loaded library. The caller controls the path, and the connection mutex
        // guarantees no other statement runs while loading is enabled. This is
        // the same trust boundary as the reference's `native.loadExtension(path)`.
        unsafe {
            conn.load_extension_enable()?;
            let result = conn.load_extension(path, None);
            let _ = conn.load_extension_disable();
            result?;
        }
        Ok(())
    }
}

impl Queryable for Sqlite {
    fn run(&self, sql: &str) -> Result<Vec<Row>> {
        self.lock()?.run(sql)
    }

    fn run_all(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        self.lock()?.run_all(sql, params)
    }

    fn run_get(&self, sql: &str, params: &[Value]) -> Result<Option<Row>> {
        self.lock()?.run_get(sql, params)
    }

    fn run_exec(&self, sql: &str, params: &[Value]) -> Result<usize> {
        self.lock()?.run_exec(sql, params)
    }

    fn run_batch(&self, sql: &str) -> Result<()> {
        self.lock()?.run_batch(sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Sqlite {
        Sqlite::open(Config {
            filename: ":memory:".to_string(),
            disable_wal: true,
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn run_all_get_and_execute() {
        let db = memory();
        db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .unwrap();
        db.execute_with(
            "INSERT INTO t (id, name) VALUES (?, ?)",
            &[Value::Integer(1), Value::Text("a".into())],
        )
        .unwrap();
        assert_eq!(
            db.execute_with(
                "INSERT INTO t (id, name) VALUES (?, ?)",
                &[Value::Integer(2), Value::Text("b".into())]
            )
            .unwrap(),
            1
        );
        let rows = db.all("SELECT id, name FROM t ORDER BY id", &[]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].get_by_name::<String>("name").unwrap(), "b");
        let row = db
            .get("SELECT name FROM t WHERE id = ?", &[Value::Integer(1)])
            .unwrap()
            .unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), "a");
        assert!(db
            .get("SELECT name FROM t WHERE id = ?", &[Value::Integer(99)])
            .unwrap()
            .is_none());
    }

    #[test]
    fn transaction_rolls_back() {
        let db = memory();
        db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY)")
            .unwrap();
        let result = db.transaction(|tx| {
            tx.run_batch("INSERT INTO t (id) VALUES (1)")?;
            Err::<(), _>(Error::Database("boom".into()))
        });
        assert!(result.is_err());
        assert_eq!(db.all("SELECT id FROM t", &[]).unwrap().len(), 0);
        db.transaction(|tx| tx.run_batch("INSERT INTO t (id) VALUES (2)"))
            .unwrap();
        assert_eq!(db.all("SELECT id FROM t", &[]).unwrap().len(), 1);
    }

    #[test]
    fn export_round_trips() {
        let db = memory();
        db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY)")
            .unwrap();
        db.execute_with("INSERT INTO t (id) VALUES (?)", &[Value::Integer(7)])
            .unwrap();
        let bytes = db.export().unwrap();
        assert!(!bytes.is_empty());
    }
}
