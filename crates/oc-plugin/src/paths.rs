//! Global filesystem paths.
//!
//! Mirrors reference/packages/core/src/global.ts (`Global.Path`), which uses
//! the XDG base directories with the `opencode` application name.

use std::path::PathBuf;

/// The XDG-style global paths for opencode.
#[derive(Debug, Clone)]
pub struct GlobalPaths {
    pub home: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub tmp: PathBuf,
}

impl GlobalPaths {
    pub fn new() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let dirs = directories::ProjectDirs::from("", "", "opencode");
        let data = dirs
            .as_ref()
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| home.join(".local").join("share").join("opencode"));
        let cache = dirs
            .as_ref()
            .map(|d| d.cache_dir().to_path_buf())
            .unwrap_or_else(|| home.join(".cache").join("opencode"));
        let config = std::env::var_os("OPENCODE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs.as_ref().map(|d| d.config_dir().to_path_buf()))
            .unwrap_or_else(|| home.join(".config").join("opencode"));
        let state = dirs
            .as_ref()
            .and_then(|d| d.state_dir())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("state").join("opencode"));
        let tmp = std::env::temp_dir().join("opencode");
        Self {
            home,
            data,
            cache,
            config,
            state,
            tmp,
        }
    }

    /// The directory where npm plugin packages are cached. Mirrors
    /// `Npm.directory` in reference/packages/core/src/npm.ts.
    pub fn npm_packages(&self) -> PathBuf {
        self.cache.join("packages")
    }
}

impl Default for GlobalPaths {
    fn default() -> Self {
        Self::new()
    }
}
