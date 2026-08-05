//! From reference/packages/core/src/database/migration/20260504145000_add_sync_owner.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `event_sequence` ADD `owner_id` text;")?;
    Ok(())
}
