//! From reference/packages/core/src/database/migration/20260410174513_workspace-name.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("PRAGMA foreign_keys=OFF;")?;
    tx.run_batch(
        "CREATE TABLE `__new_workspace` (
          `id` text PRIMARY KEY,
          `type` text NOT NULL,
          `name` text DEFAULT '' NOT NULL,
          `branch` text,
          `directory` text,
          `extra` text,
          `project_id` text NOT NULL,
          CONSTRAINT `fk_workspace_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE
        )",
    )?;
    tx.run_batch(
        "INSERT INTO `__new_workspace`(`id`, `type`, `branch`, `name`, `directory`, `extra`, `project_id`) SELECT `id`, `type`, `branch`, `name`, `directory`, `extra`, `project_id` FROM `workspace`;",
    )?;
    tx.run_batch("DROP TABLE `workspace`;")?;
    tx.run_batch("ALTER TABLE `__new_workspace` RENAME TO `workspace`;")?;
    tx.run_batch("PRAGMA foreign_keys=ON;")?;
    Ok(())
}
