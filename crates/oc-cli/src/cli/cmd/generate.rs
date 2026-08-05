//! `opencode generate`
//! From reference/packages/opencode/src/cli/cmd/generate.ts.

use crate::cli::effect_cmd::not_wired;

pub async fn run() -> anyhow::Result<i32> {
    // TODO(integration): print the server's OpenAPI spec with SDK code samples.
    Err(not_wired(
        "openapi generation is not yet wired in this build (TODO(integration): oc-server)",
    ))
}
