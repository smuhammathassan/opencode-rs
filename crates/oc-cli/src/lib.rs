//! oc-cli — the `opencode` binary.
//!
//! Mirrors `reference/packages/opencode/src/index.ts` (entrypoint) and
//! `reference/packages/opencode/src/cli/` (command implementations).

pub mod cli;
pub mod version;

/// Version reported by `opencode --version`.
/// From reference/packages/opencode/package.json (`"version": "1.18.13"`).
pub const VERSION: &str = "1.18.13";
