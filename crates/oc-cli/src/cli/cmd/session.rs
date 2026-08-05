//! `opencode session`
//! From reference/packages/opencode/src/cli/cmd/session.ts.

use crate::cli::args::{Cli, SessionArgs, SessionCommand};
use crate::cli::effect_cmd::not_wired;

pub async fn run(_cli: &Cli, args: &SessionArgs) -> anyhow::Result<i32> {
    match &args.command {
        SessionCommand::List { max_count, format } => {
            let _ = (max_count, format);
            // TODO(integration): list sessions via `oc_database` + `oc_session`.
            Err(not_wired("session listing is not yet wired in this build (TODO(integration): oc-database/oc-session)"))
        }
        SessionCommand::Delete { session_id } => {
            let _ = session_id;
            // TODO(integration): delete via `oc_database` + `oc_session`.
            Err(not_wired("session deletion is not yet wired in this build (TODO(integration): oc-database/oc-session)"))
        }
    }
}
