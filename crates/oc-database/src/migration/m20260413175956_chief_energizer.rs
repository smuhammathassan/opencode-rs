//! From reference/packages/core/src/database/migration/20260413175956_chief_energizer.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch(
        "CREATE TABLE `session_entry` (
          `id` text PRIMARY KEY,
          `session_id` text NOT NULL,
          `type` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_session_entry_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
        )",
    )?;
    tx.run_batch("CREATE INDEX `session_entry_session_idx` ON `session_entry` (`session_id`)")?;
    tx.run_batch(
        "CREATE INDEX `session_entry_session_type_idx` ON `session_entry` (`session_id`,`type`)",
    )?;
    tx.run_batch(
        "CREATE INDEX `session_entry_time_created_idx` ON `session_entry` (`time_created`)",
    )?;
    Ok(())
}
