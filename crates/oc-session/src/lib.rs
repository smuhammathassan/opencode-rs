//! oc-session — session data model + orchestration for the opencode-rs port.
//!
//! Mirrors `reference/packages/core/src/session/` and
//! `reference/packages/opencode/src/session/` (minus the runner loop, which
//! lives in oc-session-runner). Session/message/part data models serialize to
//! JSON identical to the reference zod output; the system prompt templates are
//! embedded verbatim from `assets/prompt/*.txt`.

pub mod compaction;
pub mod compaction_core;
pub mod history;
pub mod identifier;
pub mod instruction;
pub mod llm;
pub mod message;
pub mod message_v2;
pub mod overflow;
pub mod permission;
pub mod prompt;
pub mod provider;
pub mod reminders;
pub mod retry;
pub mod revert;
pub mod run_state;
pub mod schema;
pub mod session;
pub mod status;
pub mod store;
pub mod summary;
pub mod system;
pub mod todo;
pub mod util;
pub mod v1;
pub mod v2;

/// Ordered string map preserving insertion order, used for `Record<String, _>`
/// schema fields so serialized JSON matches the reference's object key order.
pub type JsonMap = indexmap::IndexMap<String, serde_json::Value>;
