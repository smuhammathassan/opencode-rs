//! `opencode generate`
//! From reference/packages/opencode/src/cli/cmd/generate.ts.

pub async fn run() -> anyhow::Result<i32> {
    let document = oc_server::openapi::document();
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(0)
}
