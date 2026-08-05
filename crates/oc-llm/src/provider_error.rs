//! Context-overflow detection for provider error messages.
//! From reference/packages/llm/src/provider-error.ts

/// `isContextOverflow(message)`.
/// From reference/packages/llm/src/provider-error.ts
pub fn is_context_overflow(message: &str) -> bool {
    let exclusions = [
        regex::Regex::new(r"(?i)^(throttling error|service unavailable):").unwrap(),
        regex::Regex::new(r"(?i)rate limit").unwrap(),
        regex::Regex::new(r"(?i)too many requests").unwrap(),
    ];
    if exclusions.iter().any(|pattern| pattern.is_match(message)) {
        return false;
    }
    let patterns = [
        regex::Regex::new(r"(?i)prompt is too long").unwrap(),
        regex::Regex::new(r"(?i)request_too_large").unwrap(),
        regex::Regex::new(r"(?i)input is too long for requested model").unwrap(),
        regex::Regex::new(r"(?i)exceeds the context window").unwrap(),
        regex::Regex::new(r"(?i)exceeds (?:the )?(?:model'?s )?maximum context length(?: of [\d,]+ tokens?|\s*\([\d,]+\))").unwrap(),
        regex::Regex::new(r"(?i)input token count.*exceeds the maximum").unwrap(),
        regex::Regex::new(r"(?i)tokens in request more than max tokens allowed").unwrap(),
        regex::Regex::new(r"(?i)maximum prompt length is \d+").unwrap(),
        regex::Regex::new(r"(?i)reduce the length of the messages").unwrap(),
        regex::Regex::new(r"(?i)maximum context length is \d+ tokens").unwrap(),
        regex::Regex::new(r"(?i)exceeds (?:the )?maximum allowed input length of [\d,]+ tokens?").unwrap(),
        regex::Regex::new(r"(?i)input \(\d+ tokens\) is longer than the model'?s context length \(\d+ tokens\)").unwrap(),
        regex::Regex::new(r"(?i)exceeds the limit of \d+").unwrap(),
        regex::Regex::new(r"(?i)exceeds the available context size").unwrap(),
        regex::Regex::new(r"(?i)greater than the context length").unwrap(),
        regex::Regex::new(r"(?i)context window exceeds limit").unwrap(),
        regex::Regex::new(r"(?i)exceeded model token limit").unwrap(),
        regex::Regex::new(r"(?i)context[_ ]length[_ ]exceeded").unwrap(),
        regex::Regex::new(r"(?i)request entity too large").unwrap(),
        regex::Regex::new(r"(?i)context length is only \d+ tokens").unwrap(),
        regex::Regex::new(r"(?i)input length.*exceeds.*context length").unwrap(),
        regex::Regex::new(r"(?i)prompt too long; exceeded (?:max )?context length").unwrap(),
        regex::Regex::new(r"(?i)too large for model with \d+ maximum context length").unwrap(),
        regex::Regex::new(r"(?i)prompt has [\d,]+ tokens?, but the configured context size is [\d,]+ tokens?").unwrap(),
        regex::Regex::new(r"(?i)model_context_window_exceeded").unwrap(),
        regex::Regex::new(r"(?i)too many tokens").unwrap(),
        regex::Regex::new(r"(?i)token limit exceeded").unwrap(),
    ];
    let no_body = regex::Regex::new(r"(?i)^4(00|13)\s*(status code)?\s*\(no body\)").unwrap();
    patterns.iter().any(|pattern| pattern.is_match(message)) || no_body.is_match(message)
}

/// `isContextOverflowFailure(failure)`.
/// From reference/packages/llm/src/provider-error.ts
pub fn is_context_overflow_failure(failure: &crate::schema::LlmError) -> bool {
    failure.is_invalid_request()
        && failure.reason.classification()
            == Some(crate::schema::ProviderFailureClassification::ContextOverflow)
}
