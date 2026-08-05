//! From reference/packages/core/src/database/migration/20260604172448_event_sourced_session_input.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("DELETE FROM `session_input`;")?;
    tx.run_batch("DELETE FROM `session_message`;")?;
    tx.run_batch("DELETE FROM `event`;")?;
    tx.run_batch("DELETE FROM `event_sequence`;")?;
    tx.run_batch("UPDATE `session` SET `workspace_id` = NULL;")?;
    tx.run_batch("DELETE FROM `workspace`;")?;
    tx.run_batch("DROP INDEX IF EXISTS `event_aggregate_seq_idx`;")?;
    tx.run_batch(
        "CREATE UNIQUE INDEX `event_aggregate_seq_idx` ON `event` (`aggregate_id`,`seq`)",
    )?;
    tx.run_batch("DROP INDEX IF EXISTS `session_message_session_seq_idx`;")?;
    tx.run_batch("CREATE UNIQUE INDEX `session_message_session_seq_idx` ON `session_message` (`session_id`,`seq`)")?;
    tx.run_batch("PRAGMA foreign_keys=OFF;")?;
    tx.run_batch(
        "CREATE TABLE `__new_session_input` (
          `id` text PRIMARY KEY,
          `session_id` text NOT NULL,
          `prompt` text NOT NULL,
          `delivery` text NOT NULL,
          `admitted_seq` integer NOT NULL,
          `promoted_seq` integer,
          `time_created` integer NOT NULL,
          CONSTRAINT `fk_session_input_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
        )",
    )?;
    tx.run_batch("DROP TABLE `session_input`;")?;
    tx.run_batch("ALTER TABLE `__new_session_input` RENAME TO `session_input`;")?;
    tx.run_batch("PRAGMA foreign_keys=ON;")?;
    tx.run_batch(
        "CREATE INDEX `session_input_session_pending_delivery_seq_idx` ON `session_input` (`session_id`,`promoted_seq`,`delivery`,`admitted_seq`)",
    )?;
    tx.run_batch(
        "CREATE UNIQUE INDEX `session_input_session_admitted_seq_idx` ON `session_input` (`session_id`,`admitted_seq`)",
    )?;
    tx.run_batch(
        "CREATE UNIQUE INDEX `session_input_session_promoted_seq_idx` ON `session_input` (`session_id`,`promoted_seq`)",
    )?;
    Ok(())
}
