//! From reference/packages/core/src/database/migration/20260611192811_lush_chimera.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("DROP INDEX IF EXISTS `credential_connector_active_idx`;")?;
    tx.run_batch("DROP TABLE `credential`;")?;
    tx.run_batch(
        "CREATE TABLE `credential` (
          `id` text PRIMARY KEY,
          `integration_id` text,
          `label` text NOT NULL,
          `value` text NOT NULL,
          `connector_id` text,
          `method_id` text,
          `active` integer,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL
        )",
    )?;
    Ok(())
}
