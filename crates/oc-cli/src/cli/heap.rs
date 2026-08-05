//! Heap snapshot helper.
//! From reference/packages/opencode/src/cli/heap.ts.

use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// Whether `OPENCODE_AUTO_HEAP_SNAPSHOT` is enabled.
/// From reference/packages/core/src/flag/flag.ts.
fn enabled() -> bool {
    matches!(
        std::env::var("OPENCODE_AUTO_HEAP_SNAPSHOT")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "true" | "1"
    )
}

/// Mirrors `Heap.start()`. Rust has no in-process heap snapshot, so the flag is
/// honored but no snapshot is produced. Kept for CLI parity.
pub fn start() {
    if !enabled() {
        return;
    }
    tracing::warn!(
        "OPENCODE_AUTO_HEAP_SNAPSHOT is not supported in the Rust port (no V8 heap); ignoring"
    );
    // TODO(integration): emit the equivalent of a heap snapshot from the
    // process's actual memory usage (e.g. /proc/<pid>/status) at MINUTE
    // intervals while RSS > LIMIT.
    let _ = MINUTE;
    let _ = LIMIT;
}
