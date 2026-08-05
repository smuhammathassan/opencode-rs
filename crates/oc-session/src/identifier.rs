/// From reference/packages/opencode/src/id/id.ts and
/// reference/packages/schema/src/identifier.ts
///
/// Generates monotonic, sortable ids such as `ses_1f...` (26 chars total).
/// The id is `prefix + "_" + 6 hex bytes (48 bits of timestamp/counter, big
/// endian, bitwise-complemented when descending) + 14 base62 random chars`.
use std::sync::Mutex;

const LENGTH: usize = 26;
const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const PREFIXES: [(&str, &str); 10] = [
    ("job", "job"),
    ("event", "evt"),
    ("session", "ses"),
    ("message", "msg"),
    ("permission", "per"),
    ("question", "que"),
    ("part", "prt"),
    ("pty", "pty"),
    ("tool", "tool"),
    ("workspace", "wrk"),
];

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Ascending,
    Descending,
}

struct State {
    last_timestamp: u64,
    counter: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    last_timestamp: 0,
    counter: 0,
});

/// Resolve a `:prefix` key to its literal prefix string.
pub fn prefix_for(key: &str) -> Option<&'static str> {
    PREFIXES.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// From reference `id.ts:generateID` — validates a given id against the prefix.
pub fn with_given(prefix: &str, given: Option<&str>) -> Result<Option<String>, String> {
    match given {
        None => Ok(None),
        Some(value) => {
            if !value.starts_with(prefix) {
                return Err(format!("ID {value} does not start with {prefix}"));
            }
            Ok(Some(value.to_string()))
        }
    }
}

/// From reference `id.ts:ascending` / `schema/identifier.ts:ascending`.
pub fn ascending(prefix: &str, given: Option<&str>) -> Result<String, String> {
    match with_given(prefix, given)? {
        Some(value) => Ok(value),
        None => Ok(create(prefix, Direction::Ascending, None)),
    }
}

/// From reference `id.ts:descending` / `schema/identifier.ts:descending`.
pub fn descending(prefix: &str, given: Option<&str>) -> Result<String, String> {
    match with_given(prefix, given)? {
        Some(value) => Ok(value),
        None => Ok(create(prefix, Direction::Descending, None)),
    }
}

/// From reference `id.ts:create`. Timestamp defaults to now; each call within
/// the same millisecond bumps a counter, so ids sort by creation time.
pub fn create(prefix: &str, direction: Direction, timestamp: Option<u64>) -> String {
    let now = timestamp.unwrap_or_else(now_millis);
    let mut state = STATE.lock().expect("identifier state poisoned");
    if now != state.last_timestamp {
        state.last_timestamp = now;
        state.counter = 0;
    }
    state.counter += 1;

    let current = now.wrapping_mul(0x1000).wrapping_add(state.counter);
    let value = match direction {
        Direction::Ascending => current,
        // `~now` over BigInt is -(now+1); in the low 48 bits that is the
        // bitwise complement of the ascending bytes (see reference comment).
        Direction::Descending => !current,
    };

    let mut time = String::with_capacity(12);
    for index in 0..6 {
        let byte = ((value >> (40 - 8 * index)) & 0xff) as u8;
        time.push_str(&format!("{byte:02x}"));
    }
    let random = random_base62(LENGTH - 12);
    format!("{prefix}_{time}{random}")
}

fn random_base62(length: usize) -> String {
    let bytes = rand_bytes(length);
    bytes
        .iter()
        .map(|byte| CHARS[*byte as usize % 62] as char)
        .collect()
}

fn rand_bytes(length: usize) -> Vec<u8> {
    // No `rand`/`getrandom` dependency: draw from the OS CSPRNG. Fall back to
    // a time/address-seeded xorshift when the OS source is unavailable (tests,
    // sandboxes) — ids must still be *sortable*; uniqueness is best-effort.
    let mut bytes = vec![0u8; length];
    if fill_os_random(&mut bytes) {
        return bytes;
    }
    let mut seed = now_millis() ^ (std::process::id() as u64).wrapping_mul(0x9E3779B97F4A7C15);
    for byte in bytes.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *byte = (seed & 0xff) as u8;
    }
    bytes
}

fn fill_os_random(bytes: &mut [u8]) -> bool {
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        return file.read_exact(bytes).is_ok();
    }
    false
}

/// From reference `id.ts:timestamp` — extract the creation timestamp from an
/// ascending id. Does not work with descending ids.
pub fn timestamp(id: &str) -> u64 {
    let prefix = id.split('_').next().unwrap_or("");
    let hex = id.get(prefix.len() + 1..prefix.len() + 13).unwrap_or("0");
    u64::from_str_radix(hex, 16).unwrap_or(0) / 0x1000
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascending_ids_sort_chronologically() {
        // timestamps below 2^36 ms (~1972) round-trip through the 48-bit field
        let a = create("msg", Direction::Ascending, Some(1_000_000));
        let b = create("msg", Direction::Ascending, Some(1_000_001));
        assert!(a < b, "{} should sort before {}", a, b);
        assert_eq!(timestamp(&a), 1_000_000);
        assert_eq!(a.len(), 30);
    }

    #[test]
    fn same_timestamp_counter_bumps() {
        let a = create("msg", Direction::Ascending, Some(1_700_000_000_000));
        let b = create("msg", Direction::Ascending, Some(1_700_000_000_000));
        assert!(a < b);
    }

    #[test]
    fn descending_ids_reverse_order() {
        let a = create("ses", Direction::Descending, Some(1_700_000_000_000));
        let b = create("ses", Direction::Descending, Some(1_700_000_000_001));
        assert!(a > b, "{} should sort after {}", a, b);
        assert!(a.starts_with("ses_"));
        assert_eq!(a.len(), 30);
    }

    #[test]
    fn given_id_is_validated() {
        assert_eq!(ascending("msg", Some("msg_abc")).unwrap(), "msg_abc");
        assert!(ascending("msg", Some("foo")).is_err());
    }

    #[test]
    fn prefix_matches_reference_table() {
        assert_eq!(prefix_for("session"), Some("ses"));
        assert_eq!(prefix_for("message"), Some("msg"));
        assert_eq!(prefix_for("part"), Some("prt"));
        assert_eq!(prefix_for("unknown"), None);
    }

    #[test]
    fn high_timestamp_wraps_48_bits_without_panicking() {
        let id = create("msg", Direction::Ascending, Some(u64::MAX));
        assert_eq!(id.len(), 30);
        let id2 = create("msg", Direction::Ascending, Some(u64::MAX));
        assert!(id2 > id);
    }
}
