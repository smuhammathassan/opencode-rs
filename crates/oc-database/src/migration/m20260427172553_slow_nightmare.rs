//! From reference/packages/core/src/database/migration/20260427172553_slow_nightmare.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch(
        "CREATE TABLE `session_message` (
          `id` text PRIMARY KEY,
          `session_id` text NOT NULL,
          `type` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_session_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
        )",
    )?;
    tx.run_batch("DROP INDEX IF EXISTS `session_entry_session_idx`;")?;
    tx.run_batch("DROP INDEX IF EXISTS `session_entry_session_type_idx`;")?;
    tx.run_batch("DROP INDEX IF EXISTS `session_entry_time_created_idx`;")?;
    tx.run_batch("CREATE INDEX `session_message_session_idx` ON `session_message` (`session_id`)")?;
    tx.run_batch("CREATE INDEX `session_message_session_type_idx` ON `session_message` (`session_id`,`type`)")?;
    tx.run_batch(
        "CREATE INDEX `session_message_time_created_idx` ON `session_message` (`time_created`)",
    )?;
    tx.run_batch("DROP TABLE `session_entry`;")?;
    Ok(())
}
