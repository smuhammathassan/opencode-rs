//! PTY environment service. From reference/packages/server/src/pty-environment.ts.
//!
//! Returns extra environment variables for spawned PTY sessions; the reference layer
//! returns `{}` and the plugin layer extends it. TODO(integration): oc-plugin pty env.

use std::collections::HashMap;

/// Resolve extra environment variables for a PTY spawned at `cwd`.
pub fn pty_environment(_input: &crate::location::Location) -> HashMap<String, String> {
    HashMap::new()
}
