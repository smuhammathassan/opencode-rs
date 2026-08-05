//! Trait interfaces for the collaborators the runner orchestrates.
//!
//! These mirror the service contracts in `packages/core/src/session/*` plus the
//! `@opencode-ai/llm` `LLMClient`. `oc-session`/`oc-llm`/`oc-tool`/`oc-provider`
//! are still stubs; implementers are supplied by those crates during
//! integration (`TODO(integration): implement from oc-session/oc-llm/oc-tool/oc-provider`).

use std::future::Future;
use std::pin::Pin;

use super::event::SessionEvent;
use super::message::{MessageID, SessionMessage};
use super::schema::{Location, SessionID, SessionInfo};
use crate::llm::error::LLMError;
use crate::llm::event::{LLMEvent, ToolOutput, ToolResultValue};
use crate::llm::message::{LLMRequest, Model, ToolDefinition};
use serde_json::Value;

/// Event sink for durable/live `session.next.*` events.
/// /// From reference/packages/core/src/session/event.ts
pub trait EventBus: Send + Sync {
    fn publish(&self, event: SessionEvent) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// `SessionStore` — session info and message history access.
/// /// From reference/packages/core/src/session/store.ts
pub trait SessionStore: Send + Sync {
    fn get(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = Option<SessionInfo>> + Send + '_>>;
    fn context(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = Vec<SessionMessage>> + Send + '_>>;
}

/// Delivery mode for promoted inputs.
/// /// From reference/packages/schema/src/session-input.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Steer,
    Queue,
}

impl Delivery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

/// `SessionInput` — durable prompt admission/promotion.
/// /// From reference/packages/core/src/session/input.ts
pub trait SessionInput: Send + Sync {
    fn has_pending(
        &self,
        session_id: &SessionID,
        delivery: Delivery,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
    fn promote_steers(
        &self,
        session_id: &SessionID,
        cutoff: u64,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + '_>>;
    fn promote_next_queued(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
    fn latest_sequence(
        &self,
        session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + '_>>;
}

/// One projected history entry (sequence number + decoded message).
/// /// From reference/packages/core/src/session/history.ts
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub seq: u64,
    pub message: SessionMessage,
}

/// `SessionHistory` — projected history selection for the runner.
/// /// From reference/packages/core/src/session/history.ts
pub trait SessionHistory: Send + Sync {
    fn entries_for_runner(
        &self,
        session_id: &SessionID,
        baseline_seq: u64,
    ) -> Pin<Box<dyn Future<Output = Vec<HistoryEntry>> + Send + '_>>;
}

/// Privileged system context baseline assembled from registered sources.
/// /// From reference/packages/core/src/system-context/index.ts
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemContext {
    pub baseline: String,
}

/// Combine several system-context contributions into one baseline, joining
/// non-empty parts with a newline. Mirrors `SystemContext.combine`.
/// /// From reference/packages/core/src/session/runner/llm.ts
pub fn combine_contexts(items: Vec<SystemContext>) -> SystemContext {
    let baseline = items
        .into_iter()
        .map(|item| item.baseline)
        .filter(|baseline| !baseline.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    SystemContext { baseline }
}

/// `SystemContextRegistry` — loads the core system context sources.
/// /// From reference/packages/core/src/system-context/registry.ts
pub trait SystemContextRegistry: Send + Sync {
    fn load(&self) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>>;
}

/// `SkillGuidance` — guidance attached for a selected agent.
/// /// From reference/packages/core/src/skill/guidance.ts
pub trait SkillGuidance: Send + Sync {
    fn load(&self, agent: &str) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>>;
}

/// `ReferenceGuidance` — guidance attached from active references.
/// /// From reference/packages/core/src/reference/guidance.ts
pub trait ReferenceGuidance: Send + Sync {
    fn load(&self) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>>;
}

/// Prepared context epoch: the durable baseline plus its sequence boundary.
/// /// From reference/packages/core/src/session/context-epoch.ts
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedContext {
    pub baseline: String,
    pub baseline_seq: u64,
}

/// `SessionContextEpoch` — durable per-session system-context baseline.
/// /// From reference/packages/core/src/session/context-epoch.ts
pub trait SessionContextEpoch: Send + Sync {
    fn initialize(
        &self,
        session_id: &SessionID,
        context: SystemContext,
    ) -> Pin<Box<dyn Future<Output = Option<PreparedContext>> + Send + '_>>;
    fn prepare(
        &self,
        session_id: &SessionID,
        context: SystemContext,
    ) -> Pin<Box<dyn Future<Output = PreparedContext> + Send + '_>>;
}

/// `SessionCompaction` — automatic and overflow compaction triggers.
/// /// From reference/packages/core/src/session/compaction.ts
pub trait SessionCompaction: Send + Sync {
    fn compact_if_needed(
        &self,
        input: CompactionInput,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
    fn compact_after_overflow(
        &self,
        input: CompactionInput,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

/// Input to compaction decisions.
/// /// From reference/packages/core/src/session/compaction.ts
#[derive(Debug, Clone)]
pub struct CompactionInput {
    pub session_id: SessionID,
    pub entries: Vec<HistoryEntry>,
    pub model: Model,
    pub request: LLMRequest,
}

/// `Snapshot` — content-addressed filesystem captures for step diffing.
/// /// From reference/packages/core/src/snapshot.ts
pub trait Snapshots: Send + Sync {
    fn capture(&self) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>>;
    fn files(
        &self,
        from: &str,
        to: &str,
    ) -> Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + '_>>;
}

/// Selected agent projection.
/// /// From reference/packages/core/src/agent.ts
#[derive(Debug, Clone, Default)]
pub struct AgentSelection {
    pub id: String,
    pub info: Option<AgentInfo>,
}

/// The agent info fields the runner reads.
/// /// From reference/packages/schema/src/agent.ts
#[derive(Debug, Clone, Default)]
pub struct AgentInfo {
    pub system: Option<String>,
    pub steps: Option<u32>,
    pub permissions: Vec<String>,
}

/// `AgentV2` — selects an agent by id.
/// /// From reference/packages/core/src/agent.ts
pub trait Agents: Send + Sync {
    fn select(&self, id: &str) -> Pin<Box<dyn Future<Output = AgentSelection> + Send + '_>>;
}

/// Runtime Location identity.
/// /// From reference/packages/core/src/location.ts
pub trait LocationService: Send + Sync {
    fn current(&self) -> Location;
}

/// A canonical tool call surfaced by the provider stream.
/// /// From reference/packages/llm/src/schema/events.ts
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
    pub provider_executed: bool,
    pub provider_metadata: Option<crate::llm::ProviderMetadata>,
}

/// Settlement input for a recorded local tool call.
/// /// From reference/packages/core/src/tool/registry.ts
#[derive(Debug, Clone)]
pub struct ExecuteInput {
    pub session_id: SessionID,
    pub agent: String,
    pub assistant_message_id: MessageID,
    pub call: ToolCall,
}

/// Settled tool outcome (result + bounded output).
/// /// From reference/packages/core/src/tool/registry.ts
#[derive(Debug, Clone)]
pub struct Settlement {
    pub result: ToolResultValue,
    pub output: Option<ToolOutput>,
    pub output_paths: Vec<String>,
}

/// Failure produced by tool output bounding.
/// /// From reference/packages/core/src/tool-output-store.ts
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
#[error("tool output store: {0}")]
pub struct ToolOutputStoreError(pub String);

/// Failure of a recorded local tool call. The reference models permission
/// declines and question rejections as defects; here they are explicit
/// variants so the runner can distinguish user-declined settlement.
/// /// From reference/packages/core/src/session/runner/llm.ts
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolSettlementError {
    #[error(transparent)]
    OutputStore(#[from] ToolOutputStoreError),
    #[error("tool execution declined")]
    Declined,
    #[error("tool execution interrupted")]
    Interrupted,
    #[error("{0}")]
    Failed(String),
}

/// One materialized tool registry snapshot with a bound settle hook.
/// /// From reference/packages/core/src/tool/registry.ts
#[derive(Clone)]
pub struct ToolMaterialization {
    pub definitions: Vec<ToolDefinition>,
    pub settle: Arc<dyn ToolSettle>,
}

/// `ToolRegistry` — materializes tool definitions for a turn.
/// /// From reference/packages/core/src/tool/registry.ts
pub trait ToolRegistry: Send + Sync {
    fn materialize(
        &self,
        permissions: &[String],
    ) -> Pin<Box<dyn Future<Output = Option<ToolMaterialization>> + Send + '_>>;
}

/// Bound tool settlement hook captured by `materialize`.
/// /// From reference/packages/core/src/tool/registry.ts
pub trait ToolSettle: Send + Sync {
    fn settle(
        &self,
        input: ExecuteInput,
    ) -> Pin<Box<dyn Future<Output = Result<Settlement, ToolSettlementError>> + Send + '_>>;
}

impl std::fmt::Debug for ToolMaterialization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolMaterialization")
            .field("definitions", &self.definitions)
            .finish_non_exhaustive()
    }
}

use std::sync::Arc;

/// `LLMClient` — one provider turn as a stream of events.
/// /// From reference/packages/llm/src/route/client.ts
pub trait LlmClient: Send + Sync {
    fn stream(
        &self,
        request: LLMRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LLMEvent>, LLMError>> + Send + '_>>;
}
