/// From reference/packages/core/src/util/hash.ts
use sha1::Sha1;
use sha2::{Digest, Sha256};

pub struct Hash;

impl Hash {
    pub fn fast(input: &[u8]) -> String {
        hex(&Sha1::digest(input))
    }

    pub fn sha256(input: &[u8]) -> String {
        hex(&Sha256::digest(input))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
