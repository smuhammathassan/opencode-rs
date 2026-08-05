//! From reference/packages/core/src/database/migration/20260211171708_add_project_commands.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `project` ADD `commands` text;")?;
    Ok(())
}
