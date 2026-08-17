//! Installation version and channel.
//! From reference/packages/core/src/installation/version.ts
//!
//! In the reference these are compile-time defines. Rust uses
//! `CARGO_PKG_VERSION`; the channel is read from an optional env override.

/// `InstallationVersion`.
pub const INSTALLATION_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `InstallationChannel` — `local` unless `OPENCODE_CHANNEL` is set at build
/// time via a `build.rs`. The reference embeds the channel as a global.
pub const INSTALLATION_CHANNEL: &str = {
    match option_env!("OPENCODE_CHANNEL") {
        Some(channel) => channel,
        None => "local",
    }
};

/// `InstallationLocal` — true when running a local build.
const fn is_local_channel() -> bool {
    let bytes = INSTALLATION_CHANNEL.as_bytes();
    bytes.len() == 5
        && bytes[0] == b'l'
        && bytes[1] == b'o'
        && bytes[2] == b'c'
        && bytes[3] == b'a'
        && bytes[4] == b'l'
}

pub const INSTALLATION_LOCAL: bool = is_local_channel();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_build_by_default() {
        assert_eq!(INSTALLATION_CHANNEL, "local");
        const { assert!(INSTALLATION_LOCAL) };
        assert!(!INSTALLATION_VERSION.is_empty());
    }
}
