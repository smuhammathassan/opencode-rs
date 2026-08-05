//! From reference/packages/core/src/database/migration/20260423070820_add_icon_url_override.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch(
        "ALTER TABLE `project` ADD `icon_url_override` text;
        UPDATE `project` SET `icon_url_override` = `icon_url` WHERE `icon_url` IS NOT NULL;",
    )?;
    Ok(())
}
