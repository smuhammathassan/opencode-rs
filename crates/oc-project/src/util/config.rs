/// Minimal Config + RuntimeFlags port used by Project/Snapshot.
///
/// `Config.get().snapshot !== false` gates snapshot tracking
/// (reference/packages/core/src/v1/config/config.ts:52); `RuntimeFlags` carries
/// `experimentalIconDiscovery` (reference/packages/opencode/src/effect/runtime-flags.ts).
///
/// TODO(integration): replace with oc-config once it lands.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub snapshot: Option<bool>,
    pub experimental_icon_discovery: bool,
}

impl Config {
    pub fn snapshot_enabled(&self) -> bool {
        self.snapshot != Some(false)
    }
}
