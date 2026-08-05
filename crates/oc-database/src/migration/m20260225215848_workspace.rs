//! From reference/packages/core/src/database/migration/20260225215848_workspace.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch(
        "CREATE TABLE `workspace` (
          `id` text PRIMARY KEY,
          `branch` text,
          `project_id` text NOT NULL,
          `config` text NOT NULL,
          CONSTRAINT `fk_workspace_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE
        )",
    )?;
    Ok(())
}
