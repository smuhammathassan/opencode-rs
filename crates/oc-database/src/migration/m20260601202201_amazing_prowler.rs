//! From reference/packages/core/src/database/migration/20260601202201_amazing_prowler.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("DROP TABLE `permission`;")?;
    Ok(())
}
