//! Sync SQL tables.
//!
//! From reference/packages/core/src/event/sql.ts and
//! reference/packages/core/src/control-plane/workspace.sql.ts.
//!
//! The DDL here is generated to match what `drizzle-kit` emits for the
//! `sqliteTable(...)` definitions in the reference (column modifier order,
//! `statement-breakpoint` separators, index DDL). The reference has no checked-in
//! migration for these tables, so the golden strings are derived from the
//! drizzle-kit `SQLiteCreateTableConvertor` / `SQLiteCreateIndexConvertor`
//! renderers (see drizzle-orm repo).
//!
//! TODO(integration): oc-database will own the real SQLite executor; this module
//! keeps the DDL + column metadata so the two stay in sync.

use std::fmt::Write;

/// A SQLite column definition, mirroring one entry of a `sqliteTable(...)`
/// field object.
#[derive(Debug, Clone)]
pub struct Column {
    pub name: &'static str,
    /// SQLite storage type, e.g. `text`, `integer`.
    pub ty: &'static str,
    pub not_null: bool,
    pub primary_key: bool,
    /// Rendered SQL literal for a DB default, e.g. `''`.
    pub default: Option<&'static str>,
    /// Foreign key target `(table, column)` plus the action for `ON DELETE`
    /// (`ON UPDATE` defaults to `no action` in drizzle-kit output).
    pub references: Option<(&'static str, &'static str, &'static str)>,
}

/// A SQLite index, mirroring a `.uniqueIndex(name)` / `.index(name)` entry.
#[derive(Debug, Clone)]
pub struct Index {
    pub name: &'static str,
    pub columns: &'static [&'static str],
    pub unique: bool,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub name: &'static str,
    pub columns: Vec<Column>,
    pub indexes: Vec<Index>,
}

impl Table {
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        writeln!(out, "CREATE TABLE `{}` (", self.name).unwrap();
        let mut lines: Vec<String> = Vec::new();
        for column in &self.columns {
            let mut line = format!("\t`{}` {}", column.name, column.ty);
            if column.primary_key {
                line.push_str(" PRIMARY KEY");
            }
            if let Some(default) = column.default {
                line.push_str(&format!(" DEFAULT {default}"));
            }
            if column.not_null {
                line.push_str(" NOT NULL");
            }
            lines.push(line);
        }
        // drizzle-kit renders all foreign keys after the column list.
        for column in &self.columns {
            if let Some((ref_table, ref_column, on_delete)) = column.references {
                lines.push(format!(
                    "\tFOREIGN KEY (`{}`) REFERENCES `{}`(`{}`) ON UPDATE no action ON DELETE {}",
                    column.name, ref_table, ref_column, on_delete
                ));
            }
        }
        out.push_str(&lines.join(",\n"));
        out.push_str("\n);\n");
        out
    }

    fn render_index(&self, index: &Index) -> String {
        let kind = if index.unique {
            "UNIQUE INDEX"
        } else {
            "INDEX"
        };
        let columns = index
            .columns
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "CREATE {kind} `{}` ON `{}` ({columns});\n",
            index.name, self.name
        )
    }
}

/// The `event_sequence` table from reference/packages/core/src/event/sql.ts.
pub fn event_sequence_table() -> Table {
    Table {
        name: "event_sequence",
        columns: vec![
            Column {
                name: "aggregate_id",
                ty: "text",
                not_null: true,
                primary_key: true,
                default: None,
                references: None,
            },
            Column {
                name: "seq",
                ty: "integer",
                not_null: true,
                primary_key: false,
                default: None,
                references: None,
            },
            Column {
                name: "owner_id",
                ty: "text",
                not_null: false,
                primary_key: false,
                default: None,
                references: None,
            },
        ],
        indexes: vec![],
    }
}

/// The `event` table from reference/packages/core/src/event/sql.ts.
pub fn event_table() -> Table {
    Table {
        name: "event",
        columns: vec![
            Column {
                name: "id",
                ty: "text",
                not_null: true,
                primary_key: true,
                default: None,
                references: None,
            },
            Column {
                name: "aggregate_id",
                ty: "text",
                not_null: true,
                primary_key: false,
                default: None,
                references: Some(("event_sequence", "aggregate_id", "cascade")),
            },
            Column {
                name: "seq",
                ty: "integer",
                not_null: true,
                primary_key: false,
                default: None,
                references: None,
            },
            Column {
                name: "type",
                ty: "text",
                not_null: true,
                primary_key: false,
                default: None,
                references: None,
            },
            Column {
                name: "data",
                ty: "text",
                not_null: true,
                primary_key: false,
                default: None,
                references: None,
            },
        ],
        indexes: vec![
            Index {
                name: "event_aggregate_seq_idx",
                columns: &["aggregate_id", "seq"],
                unique: true,
            },
            Index {
                name: "event_aggregate_type_seq_idx",
                columns: &["aggregate_id", "type", "seq"],
                unique: false,
            },
        ],
    }
}

/// The `workspace` table lives in `control_plane::workspace_sql`, mirroring
/// reference/packages/core/src/control-plane/workspace.sql.ts.
///
/// Render a full `migration.sql` bundle (with `--> statement-breakpoint`
/// separators) for the given tables, in declaration order.
pub fn render_migration(tables: &[Table]) -> String {
    let mut out = String::new();
    for (i, table) in tables.iter().enumerate() {
        if i > 0 {
            out.push_str("--> statement-breakpoint\n");
        }
        out.push_str(&table.render());
        for index in &table.indexes {
            out.push_str("--> statement-breakpoint\n");
            out.push_str(&table.render_index(index));
        }
    }
    out
}

/// The full migration DDL for the sync event tables (event + event_sequence).
pub fn event_migration() -> String {
    render_migration(&[event_sequence_table(), event_table()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_sequence_ddl_matches_drizzle() {
        assert_eq!(
            event_sequence_table().render(),
            "CREATE TABLE `event_sequence` (\n\
             \t`aggregate_id` text PRIMARY KEY NOT NULL,\n\
             \t`seq` integer NOT NULL,\n\
             \t`owner_id` text\n\
             );\n"
        );
    }

    #[test]
    fn event_ddl_matches_drizzle() {
        assert_eq!(
            event_table().render(),
            "CREATE TABLE `event` (\n\
             \t`id` text PRIMARY KEY NOT NULL,\n\
             \t`aggregate_id` text NOT NULL,\n\
             \t`seq` integer NOT NULL,\n\
             \t`type` text NOT NULL,\n\
             \t`data` text NOT NULL,\n\
             \tFOREIGN KEY (`aggregate_id`) REFERENCES `event_sequence`(`aggregate_id`) ON UPDATE no action ON DELETE cascade\n\
             );\n"
        );
    }

    #[test]
    fn event_indexes_match_drizzle() {
        let mut out = String::new();
        for index in &event_table().indexes {
            out.push_str(&event_table().render_index(index));
        }
        assert_eq!(
            out,
            "CREATE UNIQUE INDEX `event_aggregate_seq_idx` ON `event` (`aggregate_id`,`seq`);\n\
             CREATE INDEX `event_aggregate_type_seq_idx` ON `event` (`aggregate_id`,`type`,`seq`);\n"
        );
    }

    #[test]
    fn event_migration_bundle() {
        assert_eq!(
            event_migration(),
            "CREATE TABLE `event_sequence` (\n\
             \t`aggregate_id` text PRIMARY KEY NOT NULL,\n\
             \t`seq` integer NOT NULL,\n\
             \t`owner_id` text\n\
             );\n\
             --> statement-breakpoint\n\
             CREATE TABLE `event` (\n\
             \t`id` text PRIMARY KEY NOT NULL,\n\
             \t`aggregate_id` text NOT NULL,\n\
             \t`seq` integer NOT NULL,\n\
             \t`type` text NOT NULL,\n\
             \t`data` text NOT NULL,\n\
             \tFOREIGN KEY (`aggregate_id`) REFERENCES `event_sequence`(`aggregate_id`) ON UPDATE no action ON DELETE cascade\n\
             );\n\
             --> statement-breakpoint\n\
             CREATE UNIQUE INDEX `event_aggregate_seq_idx` ON `event` (`aggregate_id`,`seq`);\n\
             --> statement-breakpoint\n\
             CREATE INDEX `event_aggregate_type_seq_idx` ON `event` (`aggregate_id`,`type`,`seq`);\n"
        );
    }
}
