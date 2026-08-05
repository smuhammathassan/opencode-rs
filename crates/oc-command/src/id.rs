//! Process-global monotonically increasing identifier.
//!
//! From reference/packages/schema/src/identifier.ts (`ascending`/`create`).

use std::sync::{Mutex, OnceLock};

const LENGTH: usize = 26;
const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn counter() -> &'static Mutex<(u64, u32)> {
    static COUNTER: OnceLock<Mutex<(u64, u32)>> = OnceLock::new();
    COUNTER.get_or_init(|| Mutex::new((0, 0)))
}

pub fn ascending() -> String {
    create(false, now_millis())
}

/// Mirrors `identifier.ts::create`. The value encodes
/// `timestamp * 0x1000 + counter` as 6 big-endian hex bytes followed by 14
/// random characters (length 26).
pub fn create(descending: bool, timestamp_ms: u64) -> String {
    let mut guard = counter().lock().unwrap();
    if timestamp_ms != guard.0 {
        *guard = (timestamp_ms, 0);
    }
    guard.1 += 1;
    let counter = guard.1 as u64;

    let current = timestamp_ms.wrapping_mul(0x1000).wrapping_add(counter);
    let value = if descending { !current } else { current };

    let mut time = String::with_capacity(12);
    for index in 0..6 {
        let shift = 40 - 8 * index;
        time.push_str(&format!("{:02x}", (value >> shift) & 0xff));
    }

    let mut random = Vec::with_capacity(LENGTH - 12);
    let mut buf = [0u8; LENGTH - 12];
    getrandom::getrandom(&mut buf).ok();
    for byte in buf {
        random.push(CHARS[byte as usize % CHARS.len()] as char);
    }
    format!("{time}{}", random.iter().collect::<String>())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
