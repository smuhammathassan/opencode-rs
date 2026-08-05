//! From reference/packages/core/src/database/migration/20260312043431_session_message_cursor.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("DROP INDEX IF EXISTS `message_session_idx`;")?;
    tx.run_batch("DROP INDEX IF EXISTS `part_message_idx`;")?;
    tx.run_batch(
        "CREATE INDEX `message_session_time_created_id_idx` ON `message` (`session_id`,`time_created`,`id`)",
    )?;
    tx.run_batch("CREATE INDEX `part_message_id_id_idx` ON `part` (`message_id`,`id`)")?;
    Ok(())
}
