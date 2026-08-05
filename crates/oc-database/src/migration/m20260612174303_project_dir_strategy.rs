//! From reference/packages/core/src/database/migration/20260612174303_project_dir_strategy.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `project_directory` ADD `strategy` text;")?;
    tx.run_batch("PRAGMA foreign_keys=OFF;")?;
    tx.run_batch(
        "CREATE TABLE `__new_project_directory` (
          `project_id` text NOT NULL,
          `directory` text NOT NULL,
          `type` text,
          `strategy` text,
          `time_created` integer NOT NULL,
          CONSTRAINT `project_directory_pk` PRIMARY KEY(`project_id`, `directory`),
          CONSTRAINT `fk_project_directory_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE
        )",
    )?;
    tx.run_batch(
        "INSERT INTO `__new_project_directory`(`project_id`, `directory`, `type`, `time_created`) SELECT `project_id`, `directory`, `type`, `time_created` FROM `project_directory`;",
    )?;
    tx.run_batch("DROP TABLE `project_directory`;")?;
    tx.run_batch("ALTER TABLE `__new_project_directory` RENAME TO `project_directory`;")?;
    tx.run_batch("PRAGMA foreign_keys=ON;")?;
    Ok(())
}
