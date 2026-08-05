//! From reference/packages/core/src/database/migration/20260227213759_add_session_workspace_id.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `session` ADD `workspace_id` text;")?;
    tx.run_batch("CREATE INDEX `session_workspace_idx` ON `session` (`workspace_id`)")?;
    Ok(())
}
