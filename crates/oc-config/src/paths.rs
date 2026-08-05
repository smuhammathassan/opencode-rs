// Config path discovery.
//
// From reference/packages/opencode/src/config/paths.ts

use std::path::{Path, PathBuf};

/// Returns the home directory, honoring `OPENCODE_TEST_HOME` like
/// `Global.Path.home`.
pub fn home_dir() -> PathBuf {
    if let Some(home) = std::env::var("OPENCODE_TEST_HOME").ok() {
        return PathBuf::from(home);
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/"))
        })
}

/// The xdg config directory (`Global.Path.config`): `$XDG_CONFIG_HOME/opencode`.
pub fn config_dir() -> PathBuf {
    if let Some(config_home) = std::env::var("XDG_CONFIG_HOME").ok() {
        return PathBuf::from(config_home).join("opencode");
    }
    home_dir().join(".config").join("opencode")
}

/// Walks from `start` up to `stop` (inclusive), collecting the first existing
/// match for each target in each directory. Closest-first. Mirrors
/// `FileSystem.up`.
pub fn find_up(targets: &[&str], start: &Path, stop: Option<&Path>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        for target in targets {
            let search = dir.join(target);
            if search.exists() {
                result.push(search);
            }
        }
        if let Some(stop) = stop {
            if stop == &dir {
                break;
            }
        }
        let parent = dir.parent();
        match parent {
            Some(parent) if parent != &dir => current = Some(parent.to_path_buf()),
            _ => break,
        }
    }
    result
}

/// Project config files from `directory` up to `worktree`, root-first so the
/// closest file merges last and wins.
///
/// `ConfigPaths.files` returns `up([name.jsonc, name.json]).toReversed()`.
pub fn files(name: &str, directory: &Path, worktree: Option<&Path>) -> Vec<PathBuf> {
    let jsonc = format!("{name}.jsonc");
    let json = format!("{name}.json");
    let mut found = find_up(&[&jsonc, &json], directory, worktree);
    found.reverse();
    found
}

/// Directories that may hold config: the global config dir, `.opencode` dirs
/// between `directory` and `worktree`, the home `.opencode` dir, and
/// `OPENCODE_CONFIG_DIR`.
pub fn directories(directory: &Path, worktree: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    push_unique(&mut out, config_dir());
    let disable_project_config = std::env::var("OPENCODE_DISABLE_PROJECT_CONFIG")
        .map(|value| matches!(value.to_lowercase().as_str(), "true" | "1"))
        .unwrap_or(false);
    if !disable_project_config {
        for dir in find_up(&[".opencode"], directory, worktree) {
            push_unique(&mut out, dir);
        }
    }
    for dir in find_up(&[".opencode"], &home_dir(), Some(&home_dir())) {
        push_unique(&mut out, dir);
    }
    if let Some(config_dir) = std::env::var("OPENCODE_CONFIG_DIR").ok() {
        push_unique(&mut out, PathBuf::from(config_dir));
    }
    out
}

/// `ConfigPaths.fileInDirectory(dir, name)`.
pub fn file_in_directory(dir: &Path, name: &str) -> Vec<PathBuf> {
    vec![
        dir.join(format!("{name}.json")),
        dir.join(format!("{name}.jsonc")),
    ]
}

fn push_unique(list: &mut Vec<PathBuf>, path: PathBuf) {
    if !list.contains(&path) {
        list.push(path);
    }
}
