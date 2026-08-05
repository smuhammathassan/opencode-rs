//! From reference/packages/core/src/database/migration/20260603001617_session_message_projection_indexes.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("DROP INDEX IF EXISTS `session_message_session_idx`;")?;
    tx.run_batch("DROP INDEX IF EXISTS `session_message_session_type_idx`;")?;
    tx.run_batch("CREATE INDEX `event_aggregate_seq_idx` ON `event` (`aggregate_id`,`seq`)")?;
    tx.run_batch(
        "CREATE INDEX `session_message_session_time_created_id_idx` ON `session_message` (`session_id`,`time_created`,`id`)",
    )?;
    tx.run_batch(
        "CREATE INDEX `session_message_session_type_time_created_id_idx` ON `session_message` (`session_id`,`type`,`time_created`,`id`)",
    )?;
    Ok(())
}
