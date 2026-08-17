#![allow(clippy::all)]
//! oc-session — session data model + orchestration for the opencode-rs port.
//!
//! Mirrors `reference/packages/core/src/session/` and
//! `reference/packages/opencode/src/session/` (minus the runner loop, which
//! lives in oc-session-runner). Session/message/part data models serialize to
//! JSON identical to the reference zod output; the system prompt templates are
//! embedded verbatim from `assets/prompt/*.txt`.
//!
//! ## Long tail (partial / TODO)
//!
//! - `core/session/execution/` + `run-coordinator.ts` — V2 session execution
//!   orchestration, owned by oc-session-runner. Not ported.
//! - `core/session/context-epoch.ts` — depends on the `SystemContext` algebra
//!   (core/system-context); the store hooks (`SessionDb.context_epoch_baseline`)
//!   are in place but the epoch algebra itself is TODO(integration).
//! - `core/session/input.ts` admit/promote projections — the `Admitted` model
//!   and equivalence checks are ported; the durable-write paths need the
//!   oc-database event store.
//! - `opencode/session/processor.rs` — the full event state machine is ported
//!   against [`processor::ProcessorDeps`]; retry scheduling (Effect schedule)
//!   is left to the runner.
//! - `opencode/session/tools.rs` — only the pure MCP-resource formatting is
//!   ported; registry/tool construction is oc-tool's job.

pub mod compaction;
pub mod compaction_core;
pub mod history;
pub mod identifier;
pub mod input;
pub mod instruction;
pub mod llm;
pub mod message;
pub mod message_updater;
pub mod message_v2;
pub mod overflow;
pub mod permission;
pub mod processor;
pub mod prompt;
pub mod provider;
pub mod reminders;
pub mod retry;
pub mod revert;
pub mod run_state;
pub mod schema;
pub mod service;
pub mod session;
pub mod status;
pub mod store;
pub mod summary;
pub mod system;
pub mod todo;
pub mod tools;
pub mod util;
pub mod v1;
pub mod v2;

/// Ordered string map preserving insertion order, used for `Record<String, _>`
/// schema fields so serialized JSON matches the reference's object key order.
pub type JsonMap = indexmap::IndexMap<String, serde_json::Value>;
