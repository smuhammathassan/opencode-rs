//! From reference/packages/core/src/database/migration/20260428004200_add_session_path.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `session` ADD `path` text;")?;
    Ok(())
}
