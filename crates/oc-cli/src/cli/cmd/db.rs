//! `opencode db`
//! From reference/packages/opencode/src/cli/cmd/db.ts.

use std::path::PathBuf;

use crate::cli::args::{Cli, DbArgs, DbCommand};
use crate::cli::context::Context;

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
        None if args.query.is_none() => {
            let status = std::process::Command::new("sqlite3")
                .arg(database_path(&ctx))
                .status()
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to start sqlite3 interactive shell: {error}. Pass --query '<SQL>' instead"
                    )
                })?;
            Ok(status.code().unwrap_or(1))
        }
        _ => {
            if let Some(query) = &args.query {
                let database = oc_database::Database::open(database_path(&ctx))?;
                let rows = database.db.run(query)?;
                if args.format == "json" {
                    let data = rows
                        .iter()
                        .map(oc_database::Row::to_json)
                        .collect::<Result<Vec<_>, _>>()?;
                    println!("{}", serde_json::to_string_pretty(&data)?);
                } else {
                    if let Some(row) = rows.first() {
                        println!("{}", row.column_names().join("\t"));
                    }
                    for row in rows {
                        let values = (0..row.len())
                            .map(|index| row.value(index).map(sql_value_string).unwrap_or_default())
                            .collect::<Vec<_>>();
                        println!("{}", values.join("\t"));
                    }
                }
                Ok(0)
            } else {
                unreachable!("query-less database invocation is handled above")
            }
        }
    }
}

fn sql_value_string(value: &oc_database::Value) -> String {
    match value {
        oc_database::Value::Null => String::new(),
        oc_database::Value::Integer(value) => value.to_string(),
        oc_database::Value::Real(value) => value.to_string(),
        oc_database::Value::Text(value) => value.clone(),
        oc_database::Value::Blob(value) => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(value)
        }
    }
}
