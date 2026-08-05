//! Migration runner.
//!
//! Port of `reference/packages/core/src/database/migration.ts` and
//! `reference/packages/core/src/database/migration.gen.ts`. `apply` mirrors the
//! reference's embedded-initialization logic: an empty database gets the full
//! schema plus a `migration` journal pre-filled with every migration id, a
//! database that already has a `session` table runs only the pending migrations,
//! and any other non-empty database is rejected.

use std::collections::HashSet;

use rusqlite::types::Value;
use rusqlite::Transaction;

use crate::error::{Error, Result};
use crate::schema::now_ms;
use crate::sqlite::{Queryable, Sqlite};

pub mod gen;
mod m20260127222353_familiar_lady_ursula;
mod m20260211171708_add_project_commands;
mod m20260213144116_wakeful_the_professor;
mod m20260225215848_workspace;
mod m20260227213759_add_session_workspace_id;
mod m20260228203230_blue_harpoon;
mod m20260303231226_add_workspace_fields;
mod m20260309230000_move_org_to_state;
mod m20260312043431_session_message_cursor;
mod m20260323234822_events;
mod m20260410174513_workspace_name;
mod m20260413175956_chief_energizer;
mod m20260423070820_add_icon_url_override;
mod m20260427172553_slow_nightmare;
mod m20260428004200_add_session_path;
mod m20260501142318_next_venus;
mod m20260504145000_add_sync_owner;
mod m20260507164347_add_workspace_time;
mod m20260510033149_session_usage;
mod m20260511000411_data_migration_state;
mod m20260511173437_session_metadata;
mod m20260601010001_normalize_storage_paths;
mod m20260601202201_amazing_prowler;
mod m20260602002951_lowly_union_jack;
mod m20260602182828_add_project_directories;
mod m20260603001617_session_message_projection_indexes;
mod m20260603040000_session_message_projection_order;
mod m20260603141458_session_input_inbox;
mod m20260603160727_jittery_ezekiel_stane;
mod m20260604172448_event_sourced_session_input;
mod m20260605003541_add_session_context_snapshot;
mod m20260605042240_add_context_epoch_agent;
mod m20260611035744_credential;
mod m20260611192811_lush_chimera;
mod m20260612174303_project_dir_strategy;
mod m20260622142730_simplify_session_context_epoch;
mod m20260622170816_reset_v2_session_state;
mod m20260622202450_simplify_session_input;

pub use gen::migrations;

/// Look up a migration by id (e.g. for `apply_only` with a single migration).
pub fn by_id(id: &str) -> Option<&'static Migration> {
    gen::migrations().iter().find(|m| m.id == id)
}

/// Serializes embedded initialization for one database path, mirroring the
/// reference's module-level `Semaphore.makeUnsafe(1)`.
/// From reference/packages/core/src/database/migration.ts:11
static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire the process-wide database initialization lock.
pub fn apply_lock() -> std::result::Result<std::sync::MutexGuard<'static, ()>, crate::error::Error>
{
    APPLY_LOCK.lock().map_err(|_| Error::Poisoned)
}

/// `DatabaseMigration.Migration` — `{ id, up }`.
/// From reference/packages/core/src/database/migration.ts:13
pub struct Migration {
    pub id: &'static str,
    pub up: fn(&mut Transaction<'_>) -> Result<()>,
}

pub const fn migration(id: &'static str, up: fn(&mut Transaction<'_>) -> Result<()>) -> Migration {
    Migration { id, up }
}

/// `DatabaseMigration.apply(db)`.
/// From reference/packages/core/src/database/migration.ts:18
pub fn apply(db: &Sqlite) -> Result<()> {
    let _guard = APPLY_LOCK.lock().map_err(|_| Error::Poisoned)?;
    apply_inner(db)
}

pub(crate) fn apply_inner(db: &Sqlite) -> Result<()> {
    let tables =
        db.run("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")?;
    if tables
        .iter()
        .any(|row| row.get_by_name::<String>("name").ok().as_deref() == Some("session"))
    {
        return apply_only(db, migrations());
    }
    if !tables.is_empty() {
        return Err(Error::Database(
            "Database is not empty and has no session table".to_string(),
        ));
    }
    db.transaction(|tx| {
        crate::schema::schema_up(tx)?;
        tx.run_batch(
            "CREATE TABLE \"migration\" (id TEXT PRIMARY KEY, time_completed INTEGER NOT NULL)",
        )?;
        let now = now_ms();
        for m in migrations() {
            tx.run_exec(
                "INSERT INTO \"migration\" (id, time_completed) VALUES (?, ?)",
                &[Value::Text(m.id.to_string()), Value::Integer(now)],
            )?;
        }
        Ok(())
    })
}

/// `DatabaseMigration.applyOnly(db, input)`.
/// From reference/packages/core/src/database/migration.ts:43
pub fn apply_only(db: &Sqlite, input: &[Migration]) -> Result<()> {
    db.run_exec(
        "CREATE TABLE IF NOT EXISTS \"migration\" (id TEXT PRIMARY KEY, time_completed INTEGER NOT NULL)",
        &[],
    )?;
    let mut completed = completed_ids(db)?;
    if completed.is_empty() {
        // Existing installs used Drizzle's migration journal. Seed the new
        // journal once so TypeScript migrations don't replay old SQL.
        // From reference/packages/core/src/database/migration.ts:51
        let drizzle = db.get(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '__drizzle_migrations'",
            &[],
        )?;
        if drizzle.is_some() {
            db.run_exec(
                "INSERT OR IGNORE INTO \"migration\" (id, time_completed) \
                 SELECT name, ? FROM \"__drizzle_migrations\" WHERE name IS NOT NULL",
                &[Value::Integer(now_ms())],
            )?;
            completed = completed_ids(db)?;
        }
    }

    for m in input {
        if completed.contains(m.id) {
            continue;
        }
        db.transaction(|tx| {
            (m.up)(tx)?;
            tx.run_exec(
                "INSERT INTO \"migration\" (id, time_completed) VALUES (?, ?)",
                &[Value::Text(m.id.to_string()), Value::Integer(now_ms())],
            )?;
            Ok(())
        })?;
    }
    Ok(())
}

fn completed_ids(db: &Sqlite) -> Result<HashSet<String>> {
    let rows = db.run("SELECT id FROM \"migration\"")?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get_by_name::<String>("id").ok())
        .collect())
}
