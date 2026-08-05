//! From reference/packages/core/src/database/migration/20260507164347_add_workspace_time.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `workspace` ADD `time_used` integer NOT NULL DEFAULT 0;")?;
    Ok(())
}
