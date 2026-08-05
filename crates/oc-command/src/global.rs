//! Global runtime paths mirror of `Global.Path`.
//!
//! TODO(integration): promote to oc-core and re-export from here once
//! oc-core implements `reference/packages/core/src/global.ts`.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Global {
    pub home: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub tmp: PathBuf,
    pub bin: PathBuf,
    pub log: PathBuf,
    pub repos: PathBuf,
}

impl Global {
    pub fn detect() -> Self {
        Self::with_home(home())
    }

    /// From reference/packages/core/src/global.ts (`Global.make`).
    pub fn with_home(home: PathBuf) -> Self {
        let xdg_data = xdg(home.join(".local/share"), "XDG_DATA_HOME");
        let xdg_cache = xdg(home.join(".cache"), "XDG_CACHE_HOME");
        let xdg_config = xdg(home.join(".config"), "XDG_CONFIG_HOME");
        let xdg_state = xdg(home.join(".local/state"), "XDG_STATE_HOME");
        let data = xdg_data.join("opencode");
        let cache = xdg_cache.join("opencode");
        let config = std::env::var("OPENCODE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| xdg_config.join("opencode"));
        let state = xdg_state.join("opencode");
        Self {
            home,
            bin: cache.join("bin"),
            log: data.join("log"),
            repos: data.join("repos"),
            data,
            cache,
            config,
            state,
            tmp: std::env::temp_dir().join("opencode"),
        }
    }
}

fn home() -> PathBuf {
    std::env::var("OPENCODE_TEST_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| std::env::var("HOME").map(PathBuf::from).ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(default: PathBuf, key: &str) -> PathBuf {
    std::env::var(key)
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or(default)
}
