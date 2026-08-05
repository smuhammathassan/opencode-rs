//! The session runner: one durable coding-agent Session driven to settlement.
//!
//! Ports `packages/core/src/session/runner/*`. The orchestration lives in
//! `llm.rs`; `publish_llm_event.rs` persists one provider turn incrementally,
//! and `to_llm_message.rs` lowers projected history into canonical LLM
//! messages.

pub mod llm;
pub mod max_steps;
pub mod model;
pub mod publish_llm_event;
pub mod to_llm_message;

use std::future::Future;
use std::pin::Pin;

use tokio_util::sync::CancellationToken;

use crate::session::message::MessageID;
use crate::session::services::ToolOutputStoreError;
use crate::session::SessionID;

/// `SessionRunner.RunError` — the union of failures a drain can surface.
/// /// From reference/packages/core/src/session/runner/index.ts
#[derive(Debug, Clone, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Llm(#[from] crate::llm::LLMError),
    #[error(transparent)]
    Model(#[from] model::ModelError),
    #[error("message decode failed for {session_id}/{message_id}")]
    MessageDecode {
        session_id: SessionID,
        message_id: MessageID,
    },
    #[error("context snapshot decode failed for {session_id}: {details}")]
    ContextSnapshotDecode {
        session_id: SessionID,
        details: String,
    },
    #[error("system context initialization blocked")]
    InitializationBlocked,
    #[error("session not found: {session_id}")]
    SessionNotFound { session_id: SessionID },
    #[error("session runner defect: {0}")]
    Defect(String),
    #[error("event publication failed: {0}")]
    Publish(String),
    #[error(transparent)]
    ToolOutputStore(#[from] ToolOutputStoreError),
}

/// The runner interface: drains eligible durable work for a session. Explicit
/// runs perform one provider attempt even when no work is eligible. The token
/// signals cooperative interruption (the drain equivalent of Effect interrupt).
/// /// From reference/packages/core/src/session/runner/index.ts
pub trait SessionRunner: Send + Sync {
    fn run(
        &self,
        session_id: &SessionID,
        force: bool,
        token: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), RunError>> + Send + '_>>;
}
