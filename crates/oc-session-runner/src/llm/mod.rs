//! Canonical LLM wire types, ported from `packages/llm/src/schema/`.
//!
//! These are private mirrors of the `@opencode-ai/llm` schema that `oc-session-runner`
//! consumes. `oc-llm` is still a stub; when it lands its schema types should be promoted
//! there (`TODO(integration): promote to oc-llm`).

pub mod error;
pub mod event;
pub mod message;

pub use error::{
    is_context_overflow, is_context_overflow_failure, LLMError, LLMErrorReason, RunFailure,
    ToolFailure,
};
pub use event::{
    LLMEvent, ProviderErrorEvent, ProviderFailureClassification, ToolContent, ToolOutput,
    ToolResultValue, Usage,
};
pub use message::{
    ContentPart, LLMRequest, MediaPart, Message, MessageRole, ReasoningPart, SystemPart, TextPart,
    ToolCallPart, ToolChoice, ToolDefinition, ToolResultPart,
};

/// `LLM.ProviderMetadata` — a nested record keyed by provider name.
/// /// From reference/packages/schema/src/llm.ts
pub type ProviderMetadata = serde_json::Map<String, serde_json::Value>;

/// Loose alias used by the publisher: provider metadata is often attached
/// directly to events as a flat record rather than the nested provider map.
/// From reference/packages/llm/src/schema/ids.ts
pub type Metadata = serde_json::Map<String, serde_json::Value>;
