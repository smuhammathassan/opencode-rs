//! `opencode db`
//! From reference/packages/opencode/src/cli/cmd/db.ts.

use std::path::PathBuf;

use crate::cli::args::{Cli, DbArgs, DbCommand};
use crate::cli::context::Context;
use crate::cli::effect_cmd::not_wired;

/// Mirrors `Database.path()` from
/// reference/packages/core/src/database/database.ts.
pub fn database_path(ctx: &Context) -> PathBuf {
    if let Some(db) = std::env::var_os("OPENCODE_DB") {
        let db = PathBuf::from(db);
        if db == PathBuf::from(":memory:") || db.is_absolute() {
            return db;
        }
        return ctx.paths.data.join(db);
    }
    ctx.paths.data.join("opencode.db")
}

pub async fn run(_cli: &Cli, args: &DbArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;

    match &args.command {
        Some(DbCommand::Path) => {
            println!("{}", database_path(&ctx).display());
            Ok(0)
        }
        _ => {
            if let Some(query) = &args.query {
                // TODO(integration): run `query` through `oc_database` once the
                // SQLite crate lands. `--format json` prints rows as JSON,
                // `tsv` prints a header + tab-separated rows.
                let format = &args.format;
                let _ = (format, query);
                Err(not_wired("database queries are not yet wired in this build (TODO(integration): oc-database)"))
            } else {
                // Mirrors spawning an interactive `sqlite3` shell.
                Err(not_wired("interactive sqlite3 shell is not yet wired in this build (TODO(integration): oc-database)"))
            }
        }
    }
}
