use std::sync::atomic::{AtomicU64, Ordering};

static LAST_TS: AtomicU64 = AtomicU64::new(0);
static COUNTER: AtomicU64 = AtomicU64::new(0);

const CHARS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Port of `identifier.ascending()` from the reference: a 12-hex time+counter
/// prefix followed by 14 base62 random chars (26 chars total).
/// /// From reference/packages/schema/src/identifier.ts
pub fn ascending() -> String {
    create(false)
}

/// Port of `identifier.create(descending, timestamp)`.
/// /// From reference/packages/schema/src/identifier.ts
pub fn create(descending: bool) -> String {
    let timestamp = now_millis();
    let counter = next_counter(timestamp);
    let current = (timestamp as u128) * 0x1000 + (counter as u128);
    let value = if descending { !current } else { current };
    let mut time = String::with_capacity(12);
    for index in 0..6 {
        let byte = ((value >> (40 - 8 * index)) & 0xff) as u8;
        time.push_str(&format!("{byte:02x}"));
    }
    let mut bytes = [0u8; 14];
    getrandom_bytes(&mut bytes);
    for byte in bytes {
        time.push(CHARS[(byte % 62) as usize] as char);
    }
    time
}

fn next_counter(timestamp: u64) -> u64 {
    loop {
        let last = LAST_TS.load(Ordering::Relaxed);
        if timestamp != last {
            if LAST_TS
                .compare_exchange_weak(last, timestamp, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                COUNTER.store(1, Ordering::Relaxed);
                return 1;
            }
        } else {
            let current = COUNTER.load(Ordering::Relaxed);
            if COUNTER
                .compare_exchange_weak(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return current + 1;
            }
        }
    }
}

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn getrandom_bytes(buf: &mut [u8]) {
    // Time-seeded xorshift: uniqueness is guaranteed by the atomic counter and
    // time prefix; the trailing bytes only need to look random.
    pseudo_random(buf);
}

fn pseudo_random(buf: &mut [u8]) {
    let seed = now_millis();
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    for byte in buf {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state & 0xff) as u8;
    }
}

/// Current UTC timestamp as an RFC3339 millis string, matching `DateTime.now`.
/// /// From reference/packages/effect (DateTime.now)
pub fn timestamp_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascending_id_is_26_chars() {
        let id = ascending();
        assert_eq!(id.len(), 26);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn ascending_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..1000 {
            ids.insert(ascending());
        }
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn descending_and_ascending_differ() {
        assert_ne!(create(false), create(true));
    }
}
