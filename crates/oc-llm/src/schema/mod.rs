//! Canonical schema data model for the LLM core.
//! From reference/packages/llm/src/schema/*.ts
//!
//! TODO(integration): `oc-schema` is still a stub. These types are local
//! mirrors of `reference/packages/schema/src/llm.ts` and
//! `reference/packages/llm/src/schema/*`; promote the shared data types
//! (`ContentPart`, `Message`, `ToolDefinition`, `Usage`, `LlmEvent`, …) into
//! `oc-schema` so the rest of the workspace can reuse them.

pub mod errors;
pub mod events;
pub mod ids;
pub mod messages;
pub mod options;

pub use errors::{
    AuthKind, HttpContext, HttpRateLimitDetails, HttpRequestDetails, HttpResponseDetails, LlmError,
    LlmErrorReason, ToolFailure,
};
pub use events::{
    response_complete, response_empty, response_from_events, response_reasoning_from,
    response_reduce, response_text_from, response_tool_calls_from, response_usage_from, LlmEvent,
    LlmResponse, PreparedRequest, ProviderFailureClassification, ResponseState, Usage,
};
pub use ids::{
    CacheHint, CacheHintType, FinishReason, JsonSchema, MessageRole, ModelId, ProtocolId,
    ProviderId, ProviderMetadata, RouteId, REASONING_EFFORTS, TEXT_VERBOSITY,
};
pub use messages::{
    ContentInput, ContentPart, LlmRequest, LlmRequestInput, LlmRequestPatch, MediaData, MediaPart,
    Message, MessageInput, ReasoningPart, ResponseFormat, SystemPart, SystemPartRef, TextPart,
    ToolCallPart, ToolChoice, ToolChoiceInput, ToolChoiceType, ToolContent, ToolDefinition,
    ToolFileContent, ToolOutput, ToolResultPart, ToolResultValue, ToolTextContent,
};
pub use options::{
    merge_generation_options, merge_http_options, merge_json_records, merge_provider_options,
    CachePolicy, CachePolicyMessages, CachePolicyObject, GenerationOptions, HttpOptions, Model,
    ModelCompatibility, ModelDefaults, ModelInput, ModelLimits, ModelSerializable,
    ModelToolSchemaCompatibility, ProviderOptions,
};
