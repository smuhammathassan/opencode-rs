//! From reference/packages/core/src/database/migration/20260622142730_simplify_session_context_epoch.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `session_context_epoch` DROP COLUMN `agent`;")?;
    tx.run_batch("ALTER TABLE `session_context_epoch` DROP COLUMN `replacement_seq`;")?;
    tx.run_batch("ALTER TABLE `session_context_epoch` DROP COLUMN `revision`;")?;
    Ok(())
}
