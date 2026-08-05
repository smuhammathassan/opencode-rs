//! Workspace table DDL.
//!
//! From reference/packages/core/src/control-plane/workspace.sql.ts.

use crate::sync::sql::{render_migration, Column, Table};

/// The `WorkspaceTable` definition from the reference.
pub fn workspace_table() -> Table {
    Table {
        name: "workspace",
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
                name: "type",
                ty: "text",
                not_null: true,
                primary_key: false,
                default: None,
                references: None,
            },
            Column {
                name: "name",
                ty: "text",
                not_null: true,
                primary_key: false,
                default: Some("''"),
                references: None,
            },
            Column {
                name: "branch",
                ty: "text",
                not_null: false,
                primary_key: false,
                default: None,
                references: None,
            },
            Column {
                name: "directory",
                ty: "text",
                not_null: false,
                primary_key: false,
                default: None,
                references: None,
            },
            Column {
                name: "extra",
                ty: "text",
                not_null: false,
                primary_key: false,
                default: None,
                references: None,
            },
            Column {
                name: "project_id",
                ty: "text",
                not_null: true,
                primary_key: false,
                default: None,
                references: Some(("project", "id", "cascade")),
            },
            Column {
                name: "time_used",
                ty: "integer",
                not_null: true,
                primary_key: false,
                default: None,
                references: None,
            },
        ],
        indexes: vec![],
    }
}

/// The full migration DDL for the workspace table.
pub fn workspace_migration() -> String {
    render_migration(&[workspace_table()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_ddl_matches_drizzle() {
        assert_eq!(
            workspace_table().render(),
            "CREATE TABLE `workspace` (\n\
             \t`id` text PRIMARY KEY NOT NULL,\n\
             \t`type` text NOT NULL,\n\
             \t`name` text DEFAULT '' NOT NULL,\n\
             \t`branch` text,\n\
             \t`directory` text,\n\
             \t`extra` text,\n\
             \t`project_id` text NOT NULL,\n\
             \t`time_used` integer NOT NULL,\n\
             \tFOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON UPDATE no action ON DELETE cascade\n\
             );\n"
        );
    }

    #[test]
    fn workspace_migration_bundle() {
        assert_eq!(
            workspace_migration(),
            "CREATE TABLE `workspace` (\n\
             \t`id` text PRIMARY KEY NOT NULL,\n\
             \t`type` text NOT NULL,\n\
             \t`name` text DEFAULT '' NOT NULL,\n\
             \t`branch` text,\n\
             \t`directory` text,\n\
             \t`extra` text,\n\
             \t`project_id` text NOT NULL,\n\
             \t`time_used` integer NOT NULL,\n\
             \tFOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON UPDATE no action ON DELETE cascade\n\
             );\n"
        );
    }
}
