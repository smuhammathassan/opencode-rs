//! `opencode export [sessionID]`
//! From reference/packages/opencode/src/cli/cmd/export.ts.

use crate::cli::args::{Cli, ExportArgs};
use crate::cli::effect_cmd::not_wired;

pub async fn run(_cli: &Cli, _args: &ExportArgs) -> anyhow::Result<i32> {
    // TODO(integration): read session + messages via `oc_session`, optionally
    // redacting (`--sanitize`), and write the export JSON to stdout.
    Err(not_wired(
        "session export is not yet wired in this build (TODO(integration): oc-session/oc-database)",
    ))
}
