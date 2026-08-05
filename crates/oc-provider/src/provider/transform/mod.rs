//! ProviderTransform: per-provider request rewriting.
//!
//! From reference/packages/opencode/src/provider/transform.ts.

mod message;
mod options;
mod sampling;
mod schema;
mod variants;

pub use message::{message, MessageContent, ModelMessage};
pub use options::{options, provider_options, small_options, sdk_key, OUTPUT_TOKEN_MAX};
pub use sampling::{max_output_tokens, sanitize_surrogates, temperature, top_k, top_p};
pub use schema::{sanitize_openai_schema, schema};
pub use variants::{reasoning_variants, variants};

pub type JsonMap = serde_json::Map<String, serde_json::Value>;
pub type VariantMap = indexmap::IndexMap<String, JsonMap>;

/// `include` value that requests the encrypted reasoning state.
///
/// From `INCLUDE_ENCRYPTED_REASONING` in `transform.ts`.
pub const INCLUDE_ENCRYPTED_REASONING: [&str; 1] = ["reasoning.encrypted_content"];
