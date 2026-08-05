//! From reference/packages/core/src/database/migration/20260622202450_simplify_session_input.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch("DELETE FROM `session_context_epoch`;")?;
    tx.run_batch("DELETE FROM `session_input`;")?;
    tx.run_batch("DELETE FROM `session_message`;")?;
    tx.run_batch("DELETE FROM `event`;")?;
    tx.run_batch("DELETE FROM `event_sequence`;")?;
    tx.run_batch("UPDATE `session` SET `workspace_id` = NULL WHERE `workspace_id` IS NOT NULL;")?;
    tx.run_batch("DELETE FROM `workspace`;")?;
    Ok(())
}
