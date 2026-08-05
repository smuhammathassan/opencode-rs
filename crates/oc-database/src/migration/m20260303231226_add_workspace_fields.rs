//! From reference/packages/core/src/database/migration/20260303231226_add_workspace_fields.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `workspace` ADD `type` text NOT NULL;")?;
    tx.run_batch("ALTER TABLE `workspace` ADD `name` text;")?;
    tx.run_batch("ALTER TABLE `workspace` ADD `directory` text;")?;
    tx.run_batch("ALTER TABLE `workspace` ADD `extra` text;")?;
    tx.run_batch("ALTER TABLE `workspace` DROP COLUMN `config`;")?;
    Ok(())
}
