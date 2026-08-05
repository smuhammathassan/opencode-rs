//! Instance bootstrap.
//! From reference/packages/opencode/src/cli/bootstrap.ts.

use std::path::Path;

use super::context::Context;

/// Load a project `Context` for `directory`, run `cb`, and dispose. Mirrors the
/// reference `bootstrap(directory, cb)` wrapper (instance load + provide +
/// dispose).
pub fn bootstrap<T>(
    directory: impl AsRef<Path>,
    cb: impl FnOnce(&Context) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let ctx = Context::load(directory.as_ref().to_path_buf())?;
    let result = cb(&ctx);
    // TODO(integration): dispose the loaded instance (run disposers, emit
    // `server.instance.disposed`) when oc-core lands instance lifecycle.
    result
}
