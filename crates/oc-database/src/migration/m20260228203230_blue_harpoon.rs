//! From reference/packages/core/src/database/migration/20260228203230_blue_harpoon.ts

use rusqlite::Transaction;

use crate::error::Result;
use crate::sqlite::Queryable;

pub fn up(tx: &mut Transaction<'_>) -> Result<()> {
    tx.run_batch(
        "CREATE TABLE `account` (
          `id` text PRIMARY KEY,
          `email` text NOT NULL,
          `url` text NOT NULL,
          `access_token` text NOT NULL,
          `refresh_token` text NOT NULL,
          `token_expiry` integer,
          `selected_org_id` text,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL
        )",
    )?;
    tx.run_batch(
        "CREATE TABLE `account_state` (
          `id` integer PRIMARY KEY NOT NULL,
          `active_account_id` text,
          FOREIGN KEY (`active_account_id`) REFERENCES `account`(`id`) ON UPDATE no action ON DELETE set null
        )",
    )?;
    Ok(())
}
