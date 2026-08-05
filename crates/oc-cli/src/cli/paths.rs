//! Global on-disk paths.
//! From reference/packages/core/src/global.ts.

use std::path::PathBuf;

/// Mirrors the `Global.Path` object of the reference. Resolved eagerly so every
/// consumer shares one layout.
#[derive(Debug, Clone)]
pub struct GlobalPaths {
    pub home: PathBuf,
    pub data: PathBuf,
    pub bin: PathBuf,
    pub log: PathBuf,
    pub repos: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub tmp: PathBuf,
}

fn xdg(var: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or(fallback)
}

fn home_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home);
    }
    if let Some(dir) = directories::BaseDirs::new() {
        return dir.home_dir().to_path_buf();
    }
    PathBuf::from(".")
}

fn default_global() -> GlobalPaths {
    let home = home_dir();
    let data = xdg("XDG_DATA_HOME", home.join(".local/share"));
    let cache = xdg("XDG_CACHE_HOME", home.join(".cache"));
    let config = xdg("XDG_CONFIG_HOME", home.join(".config"));
    let state = xdg("XDG_STATE_HOME", home.join(".local/state"));
    let app = "opencode";

    GlobalPaths {
        home: home.clone(),
        data: data.join(app),
        bin: cache.join(app).join("bin"),
        log: data.join(app).join("log"),
        repos: data.join(app).join("repos"),
        cache: cache.join(app),
        config: config.join(app),
        state: state.join(app),
        tmp: std::env::temp_dir().join(app),
    }
}

impl GlobalPaths {
    /// Mirrors `Global.Path` from the reference.
    pub fn load() -> Self {
        let paths = default_global();
        let config = std::env::var_os("OPENCODE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or(paths.config.clone());
        GlobalPaths { config, ..paths }
    }

    /// The home directory, honoring `OPENCODE_TEST_HOME` like the reference.
    pub fn home(&self) -> PathBuf {
        std::env::var_os("OPENCODE_TEST_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.clone())
    }

    /// Ensure the well-known global directories exist, mirroring the
    /// `await Promise.all([...mkdir...])` in global.ts.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            &self.data,
            &self.config,
            &self.state,
            &self.tmp,
            &self.log,
            &self.bin,
            &self.repos,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

impl Default for GlobalPaths {
    fn default() -> Self {
        Self::load()
    }
}
