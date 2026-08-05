//! `oc-llm` — 1:1 Rust port of `@opencode-ai/llm` (opencode v1.18.13).
//!
//! Async streaming clients that translate opencode's internal message/part
//! types into each provider's wire protocol (SSE / AWS event-stream) and
//! stream back parts (text, tool calls, reasoning) into opencode part types.
//! Includes the tool runtime, retries, error mapping, and prompt-cache policy.

pub mod cache_policy;
pub mod llm;
pub mod provider;
pub mod provider_error;
pub mod protocols;
pub mod providers;
pub mod route;
pub mod schema;
pub mod shared;
pub mod tool;
pub mod tool_runtime;

pub use cache_policy::apply_cache_policy;
pub use llm::{generate_object, generate_object_dynamic, request, request_input, update_request, RequestInput};
pub use provider_error::is_context_overflow;
pub use provider_error::is_context_overflow_failure;
pub use route::{compile, LlmClient, Route, RouteDefaults, RouteDefaultsInput, RouteModelInput, RoutePatch};
pub use schema::*;
pub use tool::{to_definitions, Tool, ToolConfig, ToolSchema};
pub use tool_runtime::{dispatch as tool_dispatch, DispatchResult, ToolSettlement};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
