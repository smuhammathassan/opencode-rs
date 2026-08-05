//! Sortable ID generation primitives.
//!
//! From reference/packages/schema/src/identifier.ts.
//!
//! IDs are 26 characters: 12 hex characters encoding `timestamp * 0x1000 +
//! counter` (or its two's-complement for descending IDs) plus 14 random
//! base62 characters. The reference keeps a module-global counter that
//! increments once per generated ID within the same millisecond.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const LENGTH: usize = 26;
const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

struct CounterState {
    last_timestamp: u64,
    counter: u64,
}

static COUNTER: Mutex<CounterState> = Mutex::new(CounterState {
    last_timestamp: 0,
    counter: 0,
});

/// Mirrors `Date.now()`.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Mirrors `Identifier.ascending()`.
pub fn ascending() -> String {
    create(false, now_ms())
}

/// Mirrors `Identifier.descending()`.
pub fn descending() -> String {
    create(true, now_ms())
}

/// Mirrors `Identifier.create(descending, timestamp = Date.now())`.
pub fn create(descending: bool, timestamp: u64) -> String {
    let counter = {
        let mut state = COUNTER.lock().expect("id counter poisoned");
        if timestamp != state.last_timestamp {
            state.last_timestamp = timestamp;
            state.counter = 0;
        }
        state.counter += 1;
        state.counter
    };
    create_with_counter(descending, timestamp, counter)
}

/// Pure form of [`create`] with an explicit counter, used by golden tests.
///
/// Mirrors the reference computation exactly: the 6 time bytes are
/// `(value >> (40 - 8 * i)) & 0xff` where `value` is the BigInt `current`
/// (ascending) or its bitwise complement (descending). i128 reproduces the
/// JavaScript BigInt arithmetic shift and two's-complement masking.
pub fn create_with_counter(descending: bool, timestamp: u64, counter: u64) -> String {
    let current = (timestamp as i128) * 0x1000 + (counter as i128);
    let value = if descending { !current } else { current };
    let mut time = String::with_capacity(12);
    for i in 0..6usize {
        let byte = ((value >> (40 - 8 * i as i32)) & 0xff) as u8;
        time.push_str(&format!("{byte:02x}"));
    }
    let mut bytes = [0u8; LENGTH - 12];
    getrandom::getrandom(&mut bytes).expect("system RNG for ID generation");
    let random: String = bytes
        .iter()
        .map(|byte| CHARS[*byte as usize % 62] as char)
        .collect();
    format!("{time}{random}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascending_time_bytes() {
        // timestamp 1730000000000 * 0x1000 + 1
        let id = create_with_counter(false, 1730000000000, 1);
        let (time, random) = id.split_at(12);
        assert_eq!(time, "2cc091400001");
        assert_eq!(random.len(), 14);
        assert!(random.bytes().all(|b| CHARS.contains(&b)));
    }

    #[test]
    fn descending_complements_time() {
        let ascending = create_with_counter(false, 1730000000000, 1);
        let descending = create_with_counter(true, 1730000000000, 1);
        let asc_time = u64::from_str_radix(&ascending[..12], 16).unwrap();
        let desc_time = u64::from_str_radix(&descending[..12], 16).unwrap();
        // two's complement within 48 bits
        assert_eq!(desc_time, 0xffff_ffff_ffff - asc_time);
    }

    #[test]
    fn counter_increments_within_millisecond() {
        let a = create_with_counter(false, 1000, 1);
        let b = create_with_counter(false, 1000, 2);
        assert_eq!(a[..11], b[..11]);
        assert_ne!(a, b);
    }

    #[test]
    fn ascending_wrapper_uses_prefix_format() {
        let id = ascending();
        assert_eq!(id.len(), LENGTH);
        assert!(id
            .bytes()
            .all(|b| CHARS.contains(&b) || (b'0'..=b'9').contains(&b)));
    }
}
