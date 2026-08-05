pub mod git;
pub mod schema;
pub mod util;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
