pub mod git;
pub mod identity;
pub mod lsp;
pub mod project;
pub mod runtime;
pub mod schema;
pub mod snapshot;
pub mod util;
pub mod worktree;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
