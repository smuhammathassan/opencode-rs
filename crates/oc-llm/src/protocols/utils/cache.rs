//! Shared cache-marker helpers for Anthropic and Bedrock.
//! From reference/packages/llm/src/protocols/utils/cache.ts

/// `Breakpoints`.
/// From reference/packages/llm/src/protocols/utils/cache.ts
#[derive(Debug, Clone, Default)]
pub struct Breakpoints {
    pub remaining: isize,
    pub dropped: isize,
}

/// `newBreakpoints(cap)`.
pub fn new_breakpoints(cap: usize) -> Breakpoints {
    Breakpoints {
        remaining: cap as isize,
        dropped: 0,
    }
}

/// `ttlBucket(ttlSeconds)` — `"1h"` for any `ttlSeconds >= 3600`, else `None`
/// (provider default 5m).
pub fn ttl_bucket(ttl_seconds: Option<u64>) -> Option<&'static str> {
    match ttl_seconds {
        Some(seconds) if seconds >= 3600 => Some("1h"),
        _ => None,
    }
}
