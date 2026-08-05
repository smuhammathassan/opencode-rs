//! Final database schema.
//!
//! Port of `reference/packages/core/src/database/schema.gen.ts` (the generated
//! `schema.up(tx)`) and `reference/packages/core/src/database/schema.sql.ts`
//! (the `Timestamps` helper). The statements are stored in the exact form
//! SQLite persists in `sqlite_master` (leading whitespace trimmed, no trailing
//! semicolon) so a fresh database reproduces the reference DDL byte-for-byte —
//! see the golden test in `tests/schema_golden.rs`.

use rusqlite::Transaction;

use crate::sqlite::Queryable;

/// `time_created` / `time_updated` columns shared by most tables.
/// From reference/packages/core/src/database/schema.sql.ts:3
pub const TIMESTAMP_COLUMNS: &[&str] = &["time_created", "time_updated"];

/// CREATE TABLE statements from schema.gen.ts, in reference order.
pub const TABLES: &[&str] = &[
    // workspace (reference/packages/core/src/database/schema.gen.ts:8)
    concat!(
        "CREATE TABLE `workspace` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `type` text NOT NULL,\n",
        "          `name` text DEFAULT '' NOT NULL,\n",
        "          `branch` text,\n",
        "          `directory` text,\n",
        "          `extra` text,\n",
        "          `project_id` text NOT NULL,\n",
        "          `time_used` integer NOT NULL,\n",
        "          CONSTRAINT `fk_workspace_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // data_migration (schema.gen.ts:20)
    concat!(
        "CREATE TABLE `data_migration` (\n",
        "          `name` text PRIMARY KEY,\n",
        "          `time_completed` integer NOT NULL\n",
        "        )"
    ),
    // account_state (schema.gen.ts:26)
    concat!(
        "CREATE TABLE `account_state` (\n",
        "          `id` integer PRIMARY KEY,\n",
        "          `active_account_id` text,\n",
        "          `active_org_id` text,\n",
        "          CONSTRAINT `fk_account_state_active_account_id_account_id_fk` FOREIGN KEY (`active_account_id`) REFERENCES `account`(`id`) ON DELETE SET NULL\n",
        "        )"
    ),
    // account (schema.gen.ts:34)
    concat!(
        "CREATE TABLE `account` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `email` text NOT NULL,\n",
        "          `url` text NOT NULL,\n",
        "          `access_token` text NOT NULL,\n",
        "          `refresh_token` text NOT NULL,\n",
        "          `token_expiry` integer,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL\n",
        "        )"
    ),
    // control_account (schema.gen.ts:46)
    concat!(
        "CREATE TABLE `control_account` (\n",
        "          `email` text NOT NULL,\n",
        "          `url` text NOT NULL,\n",
        "          `access_token` text NOT NULL,\n",
        "          `refresh_token` text NOT NULL,\n",
        "          `token_expiry` integer,\n",
        "          `active` integer NOT NULL,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          CONSTRAINT `control_account_pk` PRIMARY KEY(`email`, `url`)\n",
        "        )"
    ),
    // credential (schema.gen.ts:59)
    concat!(
        "CREATE TABLE `credential` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `integration_id` text,\n",
        "          `label` text NOT NULL,\n",
        "          `value` text NOT NULL,\n",
        "          `connector_id` text,\n",
        "          `method_id` text,\n",
        "          `active` integer,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL\n",
        "        )"
    ),
    // event_sequence (schema.gen.ts:72)
    concat!(
        "CREATE TABLE `event_sequence` (\n",
        "          `aggregate_id` text PRIMARY KEY,\n",
        "          `seq` integer NOT NULL,\n",
        "          `owner_id` text\n",
        "        )"
    ),
    // event (schema.gen.ts:79)
    concat!(
        "CREATE TABLE `event` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `aggregate_id` text NOT NULL,\n",
        "          `seq` integer NOT NULL,\n",
        "          `type` text NOT NULL,\n",
        "          `data` text NOT NULL,\n",
        "          CONSTRAINT `fk_event_aggregate_id_event_sequence_aggregate_id_fk` FOREIGN KEY (`aggregate_id`) REFERENCES `event_sequence`(`aggregate_id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // permission (schema.gen.ts:89)
    concat!(
        "CREATE TABLE `permission` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `project_id` text NOT NULL,\n",
        "          `action` text NOT NULL,\n",
        "          `resource` text NOT NULL,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          CONSTRAINT `fk_permission_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // project_directory (schema.gen.ts:100)
    concat!(
        "CREATE TABLE `project_directory` (\n",
        "          `project_id` text NOT NULL,\n",
        "          `directory` text NOT NULL,\n",
        "          `type` text,\n",
        "          `strategy` text,\n",
        "          `time_created` integer NOT NULL,\n",
        "          CONSTRAINT `project_directory_pk` PRIMARY KEY(`project_id`, `directory`),\n",
        "          CONSTRAINT `fk_project_directory_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // project (schema.gen.ts:111)
    concat!(
        "CREATE TABLE `project` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `worktree` text NOT NULL,\n",
        "          `vcs` text,\n",
        "          `name` text,\n",
        "          `icon_url` text,\n",
        "          `icon_url_override` text,\n",
        "          `icon_color` text,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          `time_initialized` integer,\n",
        "          `sandboxes` text NOT NULL,\n",
        "          `commands` text\n",
        "        )"
    ),
    // message (schema.gen.ts:127)
    concat!(
        "CREATE TABLE `message` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `session_id` text NOT NULL,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          `data` text NOT NULL,\n",
        "          CONSTRAINT `fk_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // part (schema.gen.ts:137)
    concat!(
        "CREATE TABLE `part` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `message_id` text NOT NULL,\n",
        "          `session_id` text NOT NULL,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          `data` text NOT NULL,\n",
        "          CONSTRAINT `fk_part_message_id_message_id_fk` FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // session_context_epoch (schema.gen.ts:148)
    concat!(
        "CREATE TABLE `session_context_epoch` (\n",
        "          `session_id` text PRIMARY KEY,\n",
        "          `baseline` text NOT NULL,\n",
        "          `snapshot` text NOT NULL,\n",
        "          `baseline_seq` integer NOT NULL,\n",
        "          CONSTRAINT `fk_session_context_epoch_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // session_input (schema.gen.ts:157)
    concat!(
        "CREATE TABLE `session_input` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `session_id` text NOT NULL,\n",
        "          `prompt` text NOT NULL,\n",
        "          `delivery` text NOT NULL,\n",
        "          `admitted_seq` integer NOT NULL,\n",
        "          `promoted_seq` integer,\n",
        "          `time_created` integer NOT NULL,\n",
        "          CONSTRAINT `fk_session_input_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // session_message (schema.gen.ts:169)
    concat!(
        "CREATE TABLE `session_message` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `session_id` text NOT NULL,\n",
        "          `type` text NOT NULL,\n",
        "          `seq` integer NOT NULL,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          `data` text NOT NULL,\n",
        "          CONSTRAINT `fk_session_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // session (schema.gen.ts:181)
    concat!(
        "CREATE TABLE `session` (\n",
        "          `id` text PRIMARY KEY,\n",
        "          `project_id` text NOT NULL,\n",
        "          `workspace_id` text,\n",
        "          `parent_id` text,\n",
        "          `slug` text NOT NULL,\n",
        "          `directory` text NOT NULL,\n",
        "          `path` text,\n",
        "          `title` text NOT NULL,\n",
        "          `version` text NOT NULL,\n",
        "          `share_url` text,\n",
        "          `summary_additions` integer,\n",
        "          `summary_deletions` integer,\n",
        "          `summary_files` integer,\n",
        "          `summary_diffs` text,\n",
        "          `metadata` text,\n",
        "          `cost` real DEFAULT 0 NOT NULL,\n",
        "          `tokens_input` integer DEFAULT 0 NOT NULL,\n",
        "          `tokens_output` integer DEFAULT 0 NOT NULL,\n",
        "          `tokens_reasoning` integer DEFAULT 0 NOT NULL,\n",
        "          `tokens_cache_read` integer DEFAULT 0 NOT NULL,\n",
        "          `tokens_cache_write` integer DEFAULT 0 NOT NULL,\n",
        "          `revert` text,\n",
        "          `permission` text,\n",
        "          `agent` text,\n",
        "          `model` text,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          `time_compacting` integer,\n",
        "          `time_archived` integer,\n",
        "          CONSTRAINT `fk_session_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // todo (schema.gen.ts:215)
    concat!(
        "CREATE TABLE `todo` (\n",
        "          `session_id` text NOT NULL,\n",
        "          `content` text NOT NULL,\n",
        "          `status` text NOT NULL,\n",
        "          `priority` text NOT NULL,\n",
        "          `position` integer NOT NULL,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          CONSTRAINT `todo_pk` PRIMARY KEY(`session_id`, `position`),\n",
        "          CONSTRAINT `fk_todo_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
    // session_share (schema.gen.ts:228)
    concat!(
        "CREATE TABLE `session_share` (\n",
        "          `session_id` text PRIMARY KEY,\n",
        "          `id` text NOT NULL,\n",
        "          `secret` text NOT NULL,\n",
        "          `url` text NOT NULL,\n",
        "          `time_created` integer NOT NULL,\n",
        "          `time_updated` integer NOT NULL,\n",
        "          CONSTRAINT `fk_session_share_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE\n",
        "        )"
    ),
];

/// CREATE INDEX statements from schema.gen.ts, in reference order.
pub const INDEXES: &[&str] = &[
    // schema.gen.ts:239
    "CREATE UNIQUE INDEX `event_aggregate_seq_idx` ON `event` (`aggregate_id`,`seq`)",
    // schema.gen.ts:240
    "CREATE INDEX `event_aggregate_type_seq_idx` ON `event` (`aggregate_id`,`type`,`seq`)",
    // schema.gen.ts:241
    "CREATE UNIQUE INDEX `permission_project_action_resource_idx` ON `permission` (`project_id`,`action`,`resource`)",
    // schema.gen.ts:244
    "CREATE INDEX `message_session_time_created_id_idx` ON `message` (`session_id`,`time_created`,`id`)",
    // schema.gen.ts:247
    "CREATE INDEX `part_message_id_id_idx` ON `part` (`message_id`,`id`)",
    // schema.gen.ts:248
    "CREATE INDEX `part_session_idx` ON `part` (`session_id`)",
    // schema.gen.ts:249
    "CREATE INDEX `session_input_session_pending_delivery_seq_idx` ON `session_input` (`session_id`,`promoted_seq`,`delivery`,`admitted_seq`)",
    // schema.gen.ts:252
    "CREATE UNIQUE INDEX `session_input_session_admitted_seq_idx` ON `session_input` (`session_id`,`admitted_seq`)",
    // schema.gen.ts:255
    "CREATE UNIQUE INDEX `session_input_session_promoted_seq_idx` ON `session_input` (`session_id`,`promoted_seq`)",
    // schema.gen.ts:258
    "CREATE UNIQUE INDEX `session_message_session_seq_idx` ON `session_message` (`session_id`,`seq`)",
    // schema.gen.ts:261
    "CREATE INDEX `session_message_session_type_seq_idx` ON `session_message` (`session_id`,`type`,`seq`)",
    // schema.gen.ts:264
    "CREATE INDEX `session_message_session_time_created_id_idx` ON `session_message` (`session_id`,`time_created`,`id`)",
    // schema.gen.ts:267
    "CREATE INDEX `session_message_time_created_idx` ON `session_message` (`time_created`)",
    // schema.gen.ts:268
    "CREATE INDEX `session_project_idx` ON `session` (`project_id`)",
    // schema.gen.ts:269
    "CREATE INDEX `session_workspace_idx` ON `session` (`workspace_id`)",
    // schema.gen.ts:270
    "CREATE INDEX `session_parent_idx` ON `session` (`parent_id`)",
    // schema.gen.ts:271
    "CREATE INDEX `todo_session_idx` ON `todo` (`session_id`)",
];

/// `schema.up(tx)` — create the full schema.
/// From reference/packages/core/src/database/schema.gen.ts:5
pub fn schema_up(tx: &mut Transaction<'_>) -> crate::error::Result<()> {
    for statement in TABLES.iter().chain(INDEXES.iter()) {
        tx.run_batch(statement)?;
    }
    Ok(())
}

/// Milliseconds since the Unix epoch, matching `Date.now()`.
/// From reference/packages/core/src/database/schema.sql.ts:6
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Helper for the golden test: run the schema statements and return the
/// `sqlite_master` dump for a fresh in-memory database.
#[doc(hidden)]
pub fn dump_master(db: &crate::sqlite::Sqlite) -> crate::error::Result<Vec<(String, String)>> {
    let rows = db.all(
        "SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type, name",
        &[],
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push((
            row.get_by_name::<String>("name")?,
            row.get_by_name::<String>("sql")?,
        ));
    }
    Ok(out)
}
