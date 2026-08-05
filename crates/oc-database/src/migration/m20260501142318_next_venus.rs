//! From reference/packages/core/src/database/migration/20260501142318_next_venus.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `session` ADD `agent` text;")?;
    tx.run_batch("ALTER TABLE `session` ADD `model` text;")?;
    Ok(())
}
