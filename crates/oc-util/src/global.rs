/// From reference/packages/core/src/global.ts
///
/// Global storage paths (xdg-based). Only the pure `Path` half is ported; the
/// Effect service wrapper is an integration concern.
use std::path::PathBuf;

fn home() -> PathBuf {
    if let Some(home) = std::env::var_os("OPENCODE_TEST_HOME") {
        return PathBuf::from(home);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home);
    }
    #[cfg(windows)]
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(profile);
    }
    PathBuf::from("/")
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(dir) => PathBuf::from(dir),
        None => home().join(fallback),
    }
}

pub mod path {
    use super::xdg;
    use std::path::PathBuf;

    pub fn home() -> PathBuf {
        super::home()
    }

    pub fn data() -> PathBuf {
        xdg("XDG_DATA_HOME", ".local/share").join("opencode")
    }

    pub fn cache() -> PathBuf {
        xdg("XDG_CACHE_HOME", ".cache").join("opencode")
    }

    pub fn config() -> PathBuf {
        xdg("XDG_CONFIG_HOME", ".config").join("opencode")
    }

    pub fn state() -> PathBuf {
        xdg("XDG_STATE_HOME", ".local/state").join("opencode")
    }

    pub fn bin() -> PathBuf {
        cache().join("bin")
    }

    pub fn log() -> PathBuf {
        data().join("log")
    }

    pub fn repos() -> PathBuf {
        data().join("repos")
    }

    pub fn tmp() -> PathBuf {
        std::env::temp_dir().join("opencode")
    }
}

#[cfg(test)]
mod tests {
    use super::path;
    use std::path::PathBuf;

    #[test]
    fn bin_is_under_cache() {
        assert_eq!(path::bin(), path::cache().join("bin"));
    }

    #[test]
    fn log_and_repos_are_under_data() {
        assert_eq!(path::log(), path::data().join("log"));
        assert_eq!(path::repos(), path::data().join("repos"));
    }

    #[test]
    fn test_home_env_override() {
        std::env::set_var("OPENCODE_TEST_HOME", "/tmp/opencode-test-home");
        assert_eq!(path::home(), PathBuf::from("/tmp/opencode-test-home"));
        std::env::remove_var("OPENCODE_TEST_HOME");
    }
}
