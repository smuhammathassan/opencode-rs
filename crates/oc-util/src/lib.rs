//! `oc-util` — pure utility helpers ported from the opencode monorepo (v1.18.13).
//!
//! Mirrors:
//! - `packages/opencode/src/util/` → [`util`]
//! - `packages/core/src/fs-util.ts` → [`fs_util`]
//! - `packages/core/src/ripgrep/` → [`ripgrep`]
//! - `packages/opencode/src/format/` → [`format`]
//! - supporting helpers from `packages/core/src/util/` and `packages/core/src/npm.ts`
//!
//! Every public item carries a `/// From reference/...` citation.

pub mod fs_util;
pub mod glob;
pub mod global;
pub mod logging;
pub mod npm;
pub mod version;
pub mod which;

pub mod format;
pub mod ripgrep;

pub mod util;

/// The version the port reports at runtime: the upstream reference version
/// (`REFERENCE_VERSION`), not the crate version, for drop-in parity.
pub fn version() -> &'static str {
    version::REFERENCE_VERSION
}
