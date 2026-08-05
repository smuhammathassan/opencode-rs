//! From reference/packages/core/src/database/migration/20260603160727_jittery_ezekiel_stane.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("DROP INDEX IF EXISTS `session_input_session_pending_seq_idx`;")?;
    tx.run_batch(
        "CREATE INDEX IF NOT EXISTS `event_aggregate_type_seq_idx` ON `event` (`aggregate_id`,`type`,`seq`)",
    )?;
    tx.run_batch(
        "CREATE INDEX IF NOT EXISTS `session_input_session_pending_delivery_seq_idx` ON `session_input` (`session_id`,`promoted_seq`,`delivery`,`seq`)",
    )?;
    tx.run_batch(
        "CREATE INDEX IF NOT EXISTS `session_message_session_time_created_id_idx` ON `session_message` (`session_id`,`time_created`,`id`)",
    )?;
    Ok(())
}
