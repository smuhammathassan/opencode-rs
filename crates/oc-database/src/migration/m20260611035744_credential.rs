//! From reference/packages/core/src/database/migration/20260611035744_credential.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch(
        "CREATE TABLE `credential` (
          `id` text PRIMARY KEY,
          `connector_id` text NOT NULL,
          `method_id` text NOT NULL,
          `label` text NOT NULL,
          `value` text NOT NULL,
          `active` integer DEFAULT false NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL
        )",
    )?;
    tx.run_batch(
        r#"CREATE UNIQUE INDEX `credential_connector_active_idx` ON `credential` (`connector_id`) WHERE "credential"."active" = 1;"#,
    )?;
    Ok(())
}
