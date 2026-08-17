#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::vec_init_then_push)]
#![allow(clippy::unnecessary_sort_by)]
#![allow(clippy::useless_borrows_in_formatting)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::bind_instead_of_map)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::self_named_constructors)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::items_after_test_module)]
#![allow(clippy::await_holding_lock)]
#![allow(clippy::unused_io_amount)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::all)]
//! oc-cli — the `opencode` binary.
//!
//! Mirrors `reference/packages/opencode/src/index.ts` (entrypoint) and
//! `reference/packages/opencode/src/cli/` (command implementations).

pub mod cli;
pub mod version;

/// Version reported by `opencode --version`.
/// From reference/packages/opencode/package.json (`"version": "1.18.13"`).
/// Re-exports the shared `oc-util` constant so every crate reports the same
/// value (RELEASE-006/RELEASE-018).
pub const VERSION: &str = oc_util::version::REFERENCE_VERSION;
