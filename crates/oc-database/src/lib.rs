//! `oc-database` — SQLite schema + migrations for the opencode storage layer.
//!
//! Rust port of `reference/packages/core/src/database/`. The storage layer is
//! SQLite backed by `rusqlite`; the Bun/Node-specific sqlite adapters
//! (`sqlite.bun.ts`, `sqlite.node.ts`) collapse into the single backend in
//! [`sqlite`].

pub mod database;
pub mod error;
pub mod migration;
pub mod path;
pub mod schema;
pub mod sqlite;
pub mod tables;

pub use database::Database;
pub use error::{Error, Result};
pub use sqlite::{Config, Row, Sqlite, Value};
