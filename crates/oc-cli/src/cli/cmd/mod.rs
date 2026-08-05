//! Subcommand dispatch.
//! Mirrors the `command(...)` registrations in `reference/packages/opencode/src/index.ts`.

pub mod acp;
pub mod agent;
pub mod attach;
pub mod completion;
pub mod console;
pub mod db;
pub mod debug;
pub mod export_cmd;
pub mod generate;
pub mod github;
pub mod import_cmd;
pub mod mcp;
pub mod models;
pub mod plug;
pub mod pr;
pub mod providers;
pub mod run;
pub mod serve;
pub mod session;
pub mod stats;
pub mod uninstall;
pub mod upgrade_cmd;
pub mod web;

use crate::cli::args::{Cli, Command};
use crate::cli::ui;

/// Dispatch a parsed CLI invocation and return the process exit code.
pub async fn dispatch(cli: &Cli) -> i32 {
    let result = match &cli.command {
        None => crate::cli::cmd::attach::run_default_tui(cli).await,
        Some(Command::Completion) => completion::run(),
        Some(Command::Acp(args)) => acp::run(cli, args).await,
        Some(Command::Mcp(args)) => mcp::run(cli, args).await,
        Some(Command::Attach(args)) => attach::run(cli, args).await,
        Some(Command::Run(args)) => run::run(cli, args).await,
        Some(Command::Debug(args)) => debug::run(cli, args).await,
        Some(Command::Providers(args)) => providers::run(cli, args).await,
        Some(Command::Agent(args)) => agent::run(cli, args).await,
        Some(Command::Upgrade(args)) => upgrade_cmd::run(cli, args).await,
        Some(Command::Uninstall(args)) => uninstall::run(cli, args).await,
        Some(Command::Serve(args)) => serve::run(cli, args).await,
        Some(Command::Web(args)) => web::run(cli, args).await,
        Some(Command::Models(args)) => models::run(cli, args).await,
        Some(Command::Stats(args)) => stats::run(cli, args).await,
        Some(Command::Export(args)) => export_cmd::run(cli, args).await,
        Some(Command::Import(args)) => import_cmd::run(cli, args).await,
        Some(Command::Github(args)) => github::run(cli, args).await,
        Some(Command::Pr(args)) => pr::run(cli, args).await,
        Some(Command::Session(args)) => session::run(cli, args).await,
        Some(Command::Plugin(args)) => plug::run(cli, args).await,
        Some(Command::Db(args)) => db::run(cli, args).await,
        Some(Command::Generate(_)) => generate::run().await,
        Some(Command::Console(args)) => console::run(cli, args).await,
    };
    match result {
        Ok(code) => code,
        Err(err) => {
            let formatted = crate::cli::error::format_error(&err);
            if let Some(formatted) = formatted {
                ui::error(&formatted);
                std::process::exit(1)
            } else {
                ui::error("Unexpected error\n");
                let _ = std::io::stderr()
                    .write_all(crate::cli::error::format_unknown_error(&err).as_bytes());
                let _ = std::io::stderr().write_all(b"\n");
                std::process::exit(1)
            }
        }
    }
}

use std::io::Write;

/// Print a user-visible error and mark a non-zero exit, mirroring the reference
/// `die(message)` helper used by the `run` handler.
pub fn die(message: &str) -> ! {
    ui::error(message);
    std::process::exit(1)
}
