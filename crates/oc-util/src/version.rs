//! Shared version metadata for the opencode-rs port.
//!
//! The port tracks three orthogonal values:
//! - [`REFERENCE_VERSION`]: the upstream opencode version this port is
//!   byte-compatible with. `opencode --version` prints exactly this string so
//!   drop-in scripts keep working (RELEASE-006, RELEASE-018).
//! - [`PORT_VERSION`]: the port's own package version (workspace Cargo.toml).
//! - Build metadata ([`GIT_COMMIT`], [`BUILD_PROFILE`], [`INSTALLATION_CHANNEL`]):
//!   provenance of the exact binary that was built (RELEASE-018).

/// The upstream opencode version this port mirrors.
/// From reference/packages/opencode/package.json (`"version": "1.18.13"`).
pub const REFERENCE_VERSION: &str = "1.18.13";

/// The port's own package version (workspace Cargo.toml, `0.1.0`).
pub const PORT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit the binary was built from, or `"unknown"` when git
/// metadata is unavailable. Emitted by `build.rs`; overridable via the
/// `GIT_COMMIT` environment variable.
pub const GIT_COMMIT: &str = env!("OC_UTIL_GIT_COMMIT");

/// The cargo build profile ("debug", "release", ...), emitted by `build.rs`
/// from cargo's `PROFILE` environment variable.
pub const BUILD_PROFILE: &str = env!("OC_UTIL_BUILD_PROFILE");

/// The installation channel. Mirrors `InstallationChannel`; `"local"` until a
/// distribution channel exists (RELEASE-009).
pub const INSTALLATION_CHANNEL: &str = "local";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_version_is_byte_parity_target() {
        assert_eq!(REFERENCE_VERSION, "1.18.13");
    }

    #[test]
    fn port_version_matches_workspace() {
        assert_eq!(PORT_VERSION, "0.1.0");
    }

    #[test]
    fn git_commit_is_non_empty() {
        assert!(!GIT_COMMIT.is_empty());
    }

    #[test]
    fn build_profile_is_non_empty() {
        assert!(!BUILD_PROFILE.is_empty());
    }
}
