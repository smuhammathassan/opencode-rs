//! Glob scanning and matching.
//! From reference/packages/core/src/util/glob.ts
//!
//! The reference delegates to the JS `glob`/`minimatch` packages. The Rust
//! `glob` crate covers the same shell-style patterns for the common cases.
//! `match` is a best-effort minimatch approximation.
//! TODO(integration): if exact minimatch parity is required, replace with a
//! dedicated minimatch port owned by oc-util.

use glob::{MatchOptions, Pattern};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub cwd: Option<String>,
    pub absolute: Option<bool>,
    pub include: Option<Include>,
    pub dot: Option<bool>,
    pub symlink: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Include {
    File,
    All,
}

fn to_match_options(options: &Options) -> MatchOptions {
    MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: !options.dot.unwrap_or(false),
    }
}

/// Mirrors `Glob.scan(pattern, options)` (async).
pub fn scan(pattern: &str, options: &Options) -> Result<Vec<String>, glob::PatternError> {
    let cwd = options.cwd.clone().unwrap_or_default();
    let full_pattern = if cwd.is_empty() {
        pattern.to_string()
    } else {
        format!("{cwd}/{pattern}")
    };
    let nodir = options.include != Some(Include::All);
    let mut paths = Vec::new();
    for entry in glob::glob_with(&full_pattern, to_match_options(options))? {
        let Ok(path) = entry else { continue };
        if nodir && path.is_dir() {
            continue;
        }
        let display = path.display().to_string();
        let value = if options.absolute.unwrap_or(false) || cwd.is_empty() {
            display
        } else {
            display
                .strip_prefix(&format!("{cwd}/"))
                .unwrap_or(&display)
                .to_string()
        };
        paths.push(value);
    }
    paths.sort();
    Ok(paths)
}

/// Mirrors `Glob.match(pattern, filepath)` via `minimatch(..., { dot: true })`.
pub fn glob_match(pattern: &str, filepath: &str) -> bool {
    let options = MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    match Pattern::new(pattern) {
        Ok(pat) => pat.matches_with(filepath, options),
        Err(_) => false,
    }
}
