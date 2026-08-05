//! From reference/packages/core/src/database/migration/20260309230000_move_org_to_state.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("ALTER TABLE `account_state` ADD `active_org_id` text;")?;
    tx.run_batch(
        "UPDATE `account_state` SET `active_org_id` = (SELECT `selected_org_id` FROM `account` WHERE `account`.`id` = `account_state`.`active_account_id`);",
    )?;
    tx.run_batch("ALTER TABLE `account` DROP COLUMN `selected_org_id`;")?;
    Ok(())
}
