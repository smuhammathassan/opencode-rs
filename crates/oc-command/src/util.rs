//! Shared helpers for command/skill/question modules.

use std::path::{Path, PathBuf};

/// Escape HTML special characters.
/// From reference/packages/opencode/src/util/html.ts
pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// `OPENCODE_*` boolean flag semantics.
/// From reference/packages/core/src/flag/flag.ts (`truthy`).
pub fn env_flag(key: &str) -> bool {
    matches!(
        std::env::var(key)
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "true" | "1"
    )
}

/// Options for [`scan`], mirroring `Glob.Options`.
/// From reference/packages/core/src/util/glob.ts.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOptions {
    pub dot: bool,
    pub follow: bool,
}

/// Glob scan with node-glob semantics used by skill discovery.
/// From reference/packages/core/src/util/glob.ts (`Glob.scan`).
pub fn scan(cwd: &Path, pattern: &str, opts: &ScanOptions) -> anyhow::Result<Vec<PathBuf>> {
    let matcher = globset::Glob::new(pattern)?.compile_matcher();
    let cwd = if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        std::env::current_dir()?.join(cwd)
    };
    let walker = walkdir::WalkDir::new(&cwd)
        .follow_links(opts.follow)
        .into_iter()
        .filter_entry(|entry| {
            opts.dot || entry.depth() == 0 || !entry.file_name().to_string_lossy().starts_with('.')
        });
    let mut results: Vec<PathBuf> = walker
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| matcher.is_match(entry.path().strip_prefix(&cwd).unwrap_or(entry.path())))
        .map(|entry| entry.path().to_path_buf())
        .collect();
    results.sort();
    Ok(results)
}

/// Walk `start` upward collecting existing `<current>/<target>` paths until
/// `stop` (inclusive).
/// From reference/packages/core/src/fs-util.ts (`FileSystem.up`).
pub fn up(start: &Path, stop: Option<&Path>, targets: &[&str]) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();
    let mut current = start.to_path_buf();
    loop {
        for target in targets {
            let search = current.join(target);
            if search.is_dir() {
                result.push(search);
            }
        }
        if stop.is_some_and(|s| s == current) {
            break;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => break,
        }
    }
    result
}

/// Config search directories for skill scanning.
/// From reference/packages/opencode/src/config/paths.ts (`ConfigPaths.directories`).
pub fn config_directories(home: &Path, directory: &Path, worktree: Option<&Path>) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();
    let config_dir = std::env::var("OPENCODE_CONFIG_DIR").ok();
    let global_config = match &config_dir {
        Some(dir) => PathBuf::from(dir),
        None => xdg_config(home).join("opencode"),
    };
    result.push(global_config);
    if !env_flag("OPENCODE_DISABLE_PROJECT_CONFIG") {
        for dir in up(directory, worktree, &[".opencode"]) {
            result.push(dir);
        }
    }
    let home_opencode = home.join(".opencode");
    if home_opencode.is_dir() {
        result.push(home_opencode);
    }
    if let Some(dir) = &config_dir {
        result.push(PathBuf::from(dir));
    }
    let mut unique: Vec<PathBuf> = Vec::new();
    for dir in result {
        if !unique.contains(&dir) {
            unique.push(dir);
        }
    }
    unique
}

fn xdg_config(home: &Path) -> PathBuf {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".config"),
    }
}
