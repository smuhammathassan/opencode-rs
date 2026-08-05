pub mod bus;
pub mod config;
pub mod diff;
pub mod fs;
pub mod global;
pub mod hash;
pub mod pathutil;
pub mod process;
pub mod slug;

/// The generic `{ code, text, stderr }` git result used throughout the
/// reference (`project.ts`, `worktree/index.ts`, `snapshot/index.ts`).
#[derive(Debug, Clone, Default)]
pub struct GitResult {
    pub code: i32,
    pub text: String,
    pub stderr: String,
}

impl GitResult {
    pub fn failure(message: String) -> Self {
        GitResult { code: 1, text: String::new(), stderr: message }
    }
}
