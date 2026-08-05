//! From reference/packages/core/src/database/migration/20260603040000_session_message_projection_order.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    // Pre-launch Session projections were written before durable event
    // persistence became unconditional, so they cannot be assigned truthful
    // aggregate order.
    tx.run_batch("DELETE FROM `session_message`;")?;
    tx.run_batch("ALTER TABLE `session_message` ADD COLUMN `seq` integer NOT NULL;")?;
    tx.run_batch("DROP INDEX IF EXISTS `session_message_session_type_time_created_id_idx`;")?;
    tx.run_batch(
        "CREATE INDEX `session_message_session_seq_idx` ON `session_message` (`session_id`,`seq`)",
    )?;
    tx.run_batch(
        "CREATE INDEX `session_message_session_type_seq_idx` ON `session_message` (`session_id`,`type`,`seq`)",
    )?;
    Ok(())
}
