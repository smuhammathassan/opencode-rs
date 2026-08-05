pub mod execution;
pub mod execution_local;
pub mod llm;
pub mod retry;
pub mod run_coordinator;
pub mod runner;
pub mod session;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
