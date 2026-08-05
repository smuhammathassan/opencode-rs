//! Port of `checksum` from `reference/packages/core/src/util/encode.ts:35`.
//!
//! FNV-1a 32-bit hash rendered in base 36. Used by websearch to pick a
//! provider deterministically per session.

pub fn checksum(content: &str) -> Option<String> {
    if content.is_empty() {
        return None;
    }
    let mut hash: u32 = 0x811c9dc5;
    for byte in content.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    Some(to_base36(hash as u64))
}

fn to_base36(value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    let mut v = value;
    while v > 0 {
        out.push(DIGITS[(v % 36) as usize]);
        v /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_checksum() {
        // Reference behavior: parse the base36 checksum as an integer and take
        // parity; the exact string is stable across runs.
        let a = checksum("session-1").unwrap();
        let b = checksum("session-1").unwrap();
        assert_eq!(a, b);
        assert!(u64::from_str_radix(&a, 36).is_ok());
    }
}
