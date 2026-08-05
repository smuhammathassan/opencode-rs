/// From reference/packages/core/src/global.ts
///
/// TODO(integration): move to oc-core; this is a minimal XDG-path port used
/// until oc-core exposes `Global.Path`.
use std::path::PathBuf;

use crate::util::pathutil;

const APP: &str = "opencode";

pub struct Global;

pub struct Paths {
    pub home: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub tmp: PathBuf,
}

impl Global {
    /// Mirrors the `OPENCODE_TEST_HOME` override and `os.homedir()` fallback of
    /// `Global.Path.home`.
    pub fn home() -> PathBuf {
        match std::env::var_os("OPENCODE_TEST_HOME") {
            Some(value) => PathBuf::from(value),
            None => home_dir(),
        }
    }

    pub fn paths() -> Paths {
        let home = Global::home();
        let data = xdg_dir("XDG_DATA_HOME", &home.join(".local/share"))
            .unwrap_or_else(|| home.join(".local/share"))
            .join(APP);
        let cache = xdg_dir("XDG_CACHE_HOME", &home.join(".cache"))
            .unwrap_or_else(|| home.join(".cache"))
            .join(APP);
        let config = xdg_dir("XDG_CONFIG_HOME", &home.join(".config"))
            .unwrap_or_else(|| home.join(".config"))
            .join(APP);
        let state = xdg_dir("XDG_STATE_HOME", &home.join(".local/state"))
            .unwrap_or_else(|| home.join(".local/state"))
            .join(APP);
        let tmp = std::env::temp_dir().join(APP);
        Paths {
            home,
            data,
            cache,
            config,
            state,
            tmp,
        }
    }
}

fn xdg_dir(key: &str, fallback: &PathBuf) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(fallback.clone()))
}

#[cfg(target_os = "windows")]
fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
        })
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Resolve the opencode data directory, ensuring it exists.
pub fn data_dir() -> std::io::Result<String> {
    let dir = Global::paths().data;
    std::fs::create_dir_all(&dir)?;
    Ok(pathutil::resolve(dir.to_str().unwrap_or_default()))
}
