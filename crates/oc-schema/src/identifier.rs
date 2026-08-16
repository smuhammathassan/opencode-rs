//! From reference/packages/schema/src/identifier.ts

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

const LENGTH: usize = 26;
const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

static COUNTERS: LazyLock<Mutex<BTreeMap<i64, u64>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static PRNG_STATE: AtomicU64 = AtomicU64::new(0);

/// `create(false)` from identifier.ts.
pub fn ascending() -> String {
    create(false, now_millis())
}

/// `create(true)` from identifier.ts.
pub fn descending() -> String {
    create(true, now_millis())
}

/// Mirror of `create(descending, timestamp)` in identifier.ts: 12 hex chars from
/// `timestamp * 0x1000 + counter`, then 14 chars drawn from the base62 alphabet.
pub fn create(descending: bool, timestamp: i64) -> String {
    let counter = {
        let mut counters = COUNTERS.lock().expect("identifier counter lock poisoned");
        let counter = counters.entry(timestamp).or_default();
        *counter += 1;
        *counter
    };
    let current = (timestamp as u64).wrapping_mul(0x1000) + counter;
    let value = if descending { !current } else { current };
    let mut result = String::with_capacity(LENGTH);
    for index in 0..6 {
        let byte = ((value >> (40 - 8 * index)) & 0xff) as u8;
        result.push_str(&format!("{byte:02x}"));
    }
    for byte in random_bytes(LENGTH - 12) {
        result.push(CHARS[byte as usize % 62] as char);
    }
    result
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Tiny xorshift64* PRNG seeded from the clock; the reference uses
/// `crypto.getRandomValues`, exact values are intentionally not reproducible.
fn random_bytes(n: usize) -> Vec<u8> {
    let mut state = PRNG_STATE.load(Ordering::SeqCst);
    if state == 0 {
        state = now_millis() as u64 ^ 0x9e3779b97f4a7c15;
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        out.push((state.wrapping_mul(0x2545F4914F6CDD1D) >> 40) as u8);
    }
    PRNG_STATE.store(state, Ordering::SeqCst);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_have_expected_shape() {
        let a = ascending();
        let b = ascending();
        assert_eq!(a.len(), 26);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(b.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn ids_are_ascending_with_timestamp() {
        let a = create(false, 1_700_000_000_000);
        let b = create(false, 1_700_000_000_000);
        let c = create(false, 1_700_000_000_001);
        // Same-millisecond IDs differ via the per-millisecond counter.
        assert_ne!(a[..12], b[..12]);
        assert_ne!(a[12..], b[12..]);
        // Later timestamps sort after earlier ones in the hex prefix.
        assert!(a[..12] < c[..12]);
    }

    #[test]
    fn descending_is_not_ascending() {
        let a = create(false, 1_700_000_000_000);
        let d = create(true, 1_700_000_000_000);
        assert_ne!(a[..12], d[..12]);
    }
}
