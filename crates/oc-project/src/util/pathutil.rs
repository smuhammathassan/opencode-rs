/// Path helpers ported from `@opencode-ai/core/fs-util` (FSUtil.contains and
/// friends) and Node's `path` module. The crate targets unix paths.
use std::path::{Path, PathBuf};

/// Mirrors Node's `path.resolve`: resolve relative input against the cwd and
/// normalize the result.
pub fn resolve(input: &str) -> String {
    let path = PathBuf::from(input);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    absolute
        .components()
        .fold(PathBuf::new(), |acc, component| match component {
            std::path::Component::CurDir => acc,
            std::path::Component::ParentDir => acc.parent().map(|p| p.to_path_buf()).unwrap_or(acc),
            other => acc.join(other.as_os_str()),
        })
        .to_string_lossy()
        .into_owned()
}

/// Normalizes a path string into slash-separated components without "..".
pub fn normalize(input: &str) -> String {
    Path::new(input)
        .components()
        .fold(PathBuf::new(), |acc, component| match component {
            std::path::Component::CurDir => acc,
            std::path::Component::ParentDir => acc.parent().map(|p| p.to_path_buf()).unwrap_or(acc),
            other => acc.join(other.as_os_str()),
        })
        .to_string_lossy()
        .into_owned()
}

/// `contains(parent, child)` from `FSUtil`: true when `child` equals `parent`
/// or is nested strictly inside it.
pub fn contains(parent: &str, child: &str) -> bool {
    let relative = relative(parent, child);
    relative.is_empty()
        || (!Path::new(&relative).is_absolute() && relative != ".." && !relative.starts_with("../"))
}

/// Mirrors Node's `path.relative`.
pub fn relative(parent: &str, child: &str) -> String {
    let parent = components(parent);
    let child = components(child);
    let mut index = 0;
    while index < parent.len() && index < child.len() && parent[index] == child[index] {
        index += 1;
    }
    if index == parent.len() && index == child.len() {
        return String::new();
    }
    let mut out: Vec<&str> = Vec::new();
    for _ in index..parent.len() {
        out.push("..");
    }
    for component in &child[index..] {
        out.push(component);
    }
    out.join("/")
}

/// Normalize drive-less absolute unix paths into their components, stripping a
/// leading root and collapsing empty segments.
fn components(input: &str) -> Vec<&str> {
    let trimmed = input.trim_end_matches(['/', '\\']);
    trimmed
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

/// Joins path parts like Node's `path.join`.
pub fn join(parts: &[&str]) -> String {
    parts.iter().fold(PathBuf::new(), |acc, part| acc.join(part)).to_string_lossy().into_owned()
}

/// Basename, matching Node's `path.basename` (no extension stripping).
pub fn basename(input: &str) -> String {
    Path::new(input)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Dirname, matching Node's `path.dirname`.
pub fn dirname(input: &str) -> String {
    Path::new(input)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Returns the absolute path with `..` resolved, like the worktree
/// `canonical` helper but without realpath.
pub fn canonicalize_fallback(input: &str) -> String {
    let normalized = normalize(input);
    if normalized.is_empty() {
        "/".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_matches_reference_semantics() {
        assert!(contains("/a/b", "/a/b"));
        assert!(contains("/a/b", "/a/b/c"));
        assert!(!contains("/a/b", "/a/bc"));
        assert!(!contains("/a/b", "/a"));
        assert!(!contains("/a", "/a/../b"));
    }

    #[test]
    fn resolve_handles_relative() {
        let resolved = resolve("/a/b/../c");
        assert_eq!(resolved, "/a/c");
    }

    #[test]
    fn relative_matches_reference() {
        assert_eq!(relative("/a/b", "/a/b"), "");
        assert_eq!(relative("/a/b", "/a/b/c"), "c");
        assert_eq!(relative("/a/b", "/a"), "..");
        assert_eq!(relative("/a/b", "/x/y"), "../../x/y");
    }
}
