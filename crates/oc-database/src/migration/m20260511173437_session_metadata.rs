//! From reference/packages/core/src/database/migration/20260511173437_session-metadata.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    // This column briefly shipped again under 20260530232709_lovely_romulus.
    let columns = tx.run("PRAGMA table_info(`session`)")?;
    if columns
        .iter()
        .any(|column| column.get_by_name::<String>("name").ok().as_deref() == Some("metadata"))
    {
        return Ok(());
    }
    tx.run_batch("ALTER TABLE `session` ADD `metadata` text;")?;
    Ok(())
}
