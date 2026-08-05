//! Hashing helpers.
//! From reference/packages/core/src/util/hash.ts

use sha1::Digest;

/// Mirrors `Hash.fast(input)` — lowercase sha1 hex.
pub fn fast(input: &[u8]) -> String {
    let mut hasher = sha1::Sha1::new();
    hasher.update(input);
    hex(&hasher.finalize())
}

/// Mirrors `Hash.sha256(input)` — lowercase sha256 hex.
pub fn sha256(input: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(input);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_known_vector() {
        assert_eq!(fast(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(fast(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
