//! `oc-cli` version helpers.
//! From reference/packages/core/src/installation/version.ts.

/// The installed opencode version. Mirrors `InstallationVersion`.
pub const INSTALLATION_VERSION: &str = crate::VERSION;

/// The installation channel. Mirrors `InstallationChannel`.
pub const INSTALLATION_CHANNEL: &str = "local";

/// Whether this is a local (dev) build. Mirrors `InstallationLocal`.
pub const INSTALLATION_LOCAL: bool = true;
