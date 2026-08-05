//! Bedrock cache-point markers.
//! From reference/packages/llm/src/protocols/utils/bedrock-cache.ts

use serde_json::{Map, Value};

use crate::protocols::utils::cache::{new_breakpoints, ttl_bucket};
use crate::schema::CacheHint;

pub use crate::protocols::utils::cache::Breakpoints;

pub const BEDROCK_BREAKPOINT_CAP: usize = 4;

/// `BedrockCache.breakpoints()`.
/// From reference/packages/llm/src/protocols/utils/bedrock-cache.ts
pub fn breakpoints() -> Breakpoints {
    new_breakpoints(BEDROCK_BREAKPOINT_CAP)
}

/// `BedrockCache.block(breakpoints, cache)` — positional `cachePoint` marker.
/// From reference/packages/llm/src/protocols/utils/bedrock-cache.ts (`block`)
pub fn block(breakpoints: &mut Breakpoints, cache: Option<&CacheHint>) -> Option<Value> {
    let cache = cache?;
    if !matches!(cache.kind, crate::schema::CacheHintType::Ephemeral | crate::schema::CacheHintType::Persistent) {
        return None;
    }
    if breakpoints.remaining <= 0 {
        breakpoints.dropped += 1;
        return None;
    }
    breakpoints.remaining -= 1;
    let mut cache_point = Map::new();
    cache_point.insert("type".to_string(), Value::String("default".to_string()));
    if ttl_bucket(cache.ttl_seconds) == Some("1h") {
        cache_point.insert("ttl".to_string(), Value::String("1h".to_string()));
    }
    Some(Value::Object(Map::from_iter([("cachePoint".to_string(), Value::Object(cache_point))])))
}
