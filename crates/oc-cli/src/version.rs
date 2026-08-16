//! `oc-cli` version helpers.
//! From reference/packages/core/src/installation/version.ts.

/// The installed opencode version. Mirrors `InstallationVersion`.
pub const INSTALLATION_VERSION: &str = oc_util::version::REFERENCE_VERSION;

/// The installation channel. Mirrors `InstallationChannel`.
pub const INSTALLATION_CHANNEL: &str = oc_util::version::INSTALLATION_CHANNEL;

/// Whether this is a local (dev) build. Mirrors `InstallationLocal`.
pub const INSTALLATION_LOCAL: bool = true;
