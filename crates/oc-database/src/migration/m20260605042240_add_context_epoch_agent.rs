//! From reference/packages/core/src/database/migration/20260605042240_add_context_epoch_agent.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `session_context_epoch` ADD `agent` text DEFAULT 'build' NOT NULL;")?;
    Ok(())
}
