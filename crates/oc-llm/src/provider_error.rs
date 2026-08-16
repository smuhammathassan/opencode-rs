//! Context-overflow detection for provider error messages.
//! From reference/packages/llm/src/provider-error.ts

/// `isContextOverflow(message)`.
/// From reference/packages/llm/src/provider-error.ts
pub use oc_provider::provider::error::is_context_overflow;

/// `isContextOverflowFailure(failure)`.
/// From reference/packages/llm/src/provider-error.ts
pub fn is_context_overflow_failure(failure: &crate::schema::LlmError) -> bool {
    failure.is_invalid_request()
        && failure.reason.classification()
            == Some(crate::schema::ProviderFailureClassification::ContextOverflow)
}
