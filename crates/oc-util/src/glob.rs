/// From reference/packages/core/src/util/glob.ts
///
/// Directory scan + minimatch-style matching. `scan` walks `cwd` and returns
/// every path (relative to `cwd`, sorted) matching the pattern, mirroring the
/// `glob` npm package with `dot: true`. `glob_match` mirrors `minimatch`
/// (`*` does not cross `/`, `**` does, `?` matches one char, `[...]` classes
/// and `{a,b}` braces are supported).
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Include {
    #[default]
    File,
    All,
}

#[derive(Clone, Default)]
pub struct Options {
    pub cwd: Option<PathBuf>,
    pub absolute: bool,
    pub include: Include,
    pub dot: bool,
    pub symlink: bool,
}

fn escape_regex_char(c: char) -> String {
    match c {
        '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        | '#' | '&' | '-' | '~' => {
            format!("\\{c}")
        }
        _ => c.to_string(),
    }
}

fn pattern_to_regex(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    if i + 2 < chars.len() && chars[i + 2] == '/' {
                        out.push_str("(?:.*/)?");
                        i += 3;
                    } else {
                        out.push_str(".*");
                        i += 2;
                    }
                } else {
                    out.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                out.push_str("[^/]");
                i += 1;
            }
            '[' => {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '!' || chars[j] == '^') {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ']' {
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    out.push_str("\\[");
                    i += 1;
                } else {
                    let mut inner: String = chars[i + 1..j].iter().collect();
                    let mut negate = false;
                    if let Some(stripped) = inner.strip_prefix(['!', '^']) {
                        negate = true;
                        inner = stripped.to_string();
                    }
                    out.push('[');
                    if negate {
                        out.push('^');
                    }
                    out.push_str(&inner);
                    out.push(']');
                    i = j + 1;
                }
            }
            '\\' => {
                if i + 1 < chars.len() {
                    out.push('\\');
                    out.push(chars[i + 1]);
                    i += 2;
                } else {
                    out.push_str("\\\\");
                    i += 1;
                }
            }
            c => {
                out.push_str(&escape_regex_char(c));
                i += 1;
            }
        }
    }
    out
}

fn build_regex(pattern: &str) -> regex::Regex {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let re = format!("^{}$", pattern_to_regex(pattern));
    regex::Regex::new(&re).unwrap_or_else(|_| regex::Regex::new("$^").unwrap())
}

fn split_braces(inner: &str) -> Vec<String> {
    let mut depth = 0u32;
    let mut parts = Vec::new();
    let mut current = String::new();
    for c in inner.chars() {
        match c {
            '{' => {
                depth += 1;
                current.push(c);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

/// Expands top-level `{a,b}` alternation groups, recursively.
pub fn expand_braces(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut depth = 0u32;
    let mut start = None;
    let mut end = None;
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let (Some(start), Some(end)) = (start, end) else {
        return vec![pattern.to_string()];
    };
    let prefix: String = chars[..start].iter().collect();
    let inner: String = chars[start + 1..end].iter().collect();
    let suffix: String = chars[end + 1..].iter().collect();
    let mut out = Vec::new();
    for alt in split_braces(&inner) {
        out.extend(expand_braces(&format!("{prefix}{alt}{suffix}")));
    }
    out
}

/// Mirrors `minimatch(filepath, pattern, { dot: true })`.
pub fn glob_match(pattern: &str, filepath: &str) -> bool {
    expand_braces(pattern)
        .iter()
        .any(|p| build_regex(p).is_match(filepath))
}

fn walk(
    root: &Path,
    rel: &str,
    options: &Options,
    pattern: &str,
    out: &mut BTreeSet<String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel_path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let file_type = entry.file_type()?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            if options.include == Include::All && glob_match(pattern, &rel_path) {
                out.insert(rel_path.clone());
            }
            let is_symlink = file_type.is_symlink();
            if !(is_symlink && !options.symlink) {
                walk(&root.join(&name), &rel_path, options, pattern, out)?;
            }
        } else if glob_match(pattern, &rel_path) {
            out.insert(rel_path);
        }
    }
    Ok(())
}

/// From reference/packages/core/src/util/glob.ts (`Glob.scanSync`).
pub fn scan_sync(pattern: &str, options: &Options) -> Result<Vec<String>, anyhow::Error> {
    let cwd = options.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
    let mut matches = BTreeSet::new();
    for pattern in expand_braces(pattern) {
        let pattern = pattern.strip_prefix("./").unwrap_or(&pattern).to_string();
        if pattern.is_empty() {
            continue;
        }
        // `foo/**` also matches the base dir `foo`, mirroring the glob package.
        if let Some(dir) = pattern.strip_suffix("/**") {
            if options.include == Include::All && cwd.join(dir).is_dir() {
                matches.insert(dir.to_string());
            }
        }
        walk(&cwd, "", options, &pattern, &mut matches)?;
    }
    let mut result: Vec<String> = matches.into_iter().collect();
    if options.absolute {
        result = result
            .iter()
            .map(|rel| cwd.join(rel).to_string_lossy().into_owned())
            .collect();
    }
    Ok(result)
}

/// From reference/packages/core/src/util/glob.ts (`Glob.scan`).
pub async fn scan(pattern: &str, options: &Options) -> Result<Vec<String>, anyhow::Error> {
    let options = options.clone();
    let pattern = pattern.to_string();
    tokio::task::spawn_blocking(move || scan_sync(&pattern, &options))
        .await
        .map_err(|e| anyhow::anyhow!("glob task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_tree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-util-glob-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/deep")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("a.txt"), "").unwrap();
        std::fs::write(dir.join("b.md"), "").unwrap();
        std::fs::write(dir.join("src/x.ts"), "").unwrap();
        std::fs::write(dir.join("src/deep/y.rs"), "").unwrap();
        std::fs::write(dir.join(".hidden/z.txt"), "").unwrap();
        std::fs::write(dir.join("src/foo.rs"), "").unwrap();
        dir
    }

    fn opts(dir: &Path) -> Options {
        Options {
            cwd: Some(dir.to_path_buf()),
            absolute: false,
            include: Include::File,
            dot: true,
            symlink: false,
        }
    }

    #[test]
    fn glob_match_star_does_not_cross_slash() {
        assert!(glob_match("*.txt", "a.txt"));
        assert!(!glob_match("*.txt", "sub/a.txt"));
        assert!(glob_match("sub/*.txt", "sub/a.txt"));
        assert!(glob_match("**/*.txt", "a.txt"));
        assert!(glob_match("**/*.txt", "sub/a.txt"));
        assert!(glob_match("src/**", "src/x.ts"));
    }

    #[test]
    fn glob_match_question_and_class() {
        assert!(glob_match("a?.txt", "ab.txt"));
        assert!(!glob_match("a?.txt", "abc.txt"));
        assert!(glob_match("file[0-9].txt", "file3.txt"));
        assert!(!glob_match("file[0-9].txt", "filex.txt"));
        assert!(glob_match("file[!0-9].txt", "filex.txt"));
    }

    #[test]
    fn glob_match_braces() {
        assert!(glob_match("{a,b}.txt", "a.txt"));
        assert!(glob_match("{a,b}.txt", "b.txt"));
        assert!(!glob_match("{a,b}.txt", "c.txt"));
        assert!(glob_match("src/**/{x,y}.rs", "src/deep/x.rs"));
    }

    #[test]
    fn glob_match_escapes_literals() {
        assert!(glob_match("a.b", "a.b"));
        assert!(!glob_match("a.b", "axb"));
    }

    #[test]
    fn scan_finds_files_recursively() {
        let dir = tmp_tree("recursive");
        let found = scan_sync("**/*.txt", &opts(&dir)).unwrap();
        assert_eq!(found, vec![".hidden/z.txt", "a.txt"]);
        let found = scan_sync("**/*.rs", &opts(&dir)).unwrap();
        assert_eq!(found, vec!["src/deep/y.rs", "src/foo.rs"]);
    }

    #[test]
    fn scan_star_matches_only_current_dir() {
        let dir = tmp_tree("star");
        let found = scan_sync("*.txt", &opts(&dir)).unwrap();
        assert_eq!(found, vec!["a.txt"]);
    }

    #[test]
    fn scan_absolute_prefixes_cwd() {
        let dir = tmp_tree("abs");
        let found = scan_sync(
            "*.txt",
            &Options {
                absolute: true,
                ..opts(&dir)
            },
        )
        .unwrap();
        assert_eq!(
            found,
            vec![dir.join("a.txt").to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn scan_include_all_returns_directories() {
        let dir = tmp_tree("all");
        let found = scan_sync(
            "**",
            &Options {
                include: Include::All,
                ..opts(&dir)
            },
        )
        .unwrap();
        assert!(found.contains(&"src".to_string()));
        assert!(found.contains(&"src/deep".to_string()));
        assert!(found.contains(&"a.txt".to_string()));
    }

    #[test]
    fn scan_does_not_include_dirs_by_default() {
        let dir = tmp_tree("nodir");
        let found = scan_sync("src/**", &opts(&dir)).unwrap();
        assert!(!found.contains(&"src".to_string()));
        assert!(found.contains(&"src/x.ts".to_string()));
    }

    #[test]
    fn scan_double_star_glob_matches_base_dir() {
        let dir = tmp_tree("basestar");
        let found = scan_sync("src/**", &opts(&dir)).unwrap();
        assert!(found.contains(&"src/x.ts".to_string()));
        assert!(found.contains(&"src/deep/y.rs".to_string()));
    }

    #[test]
    fn expand_braces_basics() {
        assert_eq!(expand_braces("{a,b}"), vec!["a", "b"]);
        assert_eq!(expand_braces("x{a,b}y"), vec!["xay", "xby"]);
        assert_eq!(expand_braces("a{b,c{d,e}}"), vec!["ab", "acd", "ace"]);
        assert_eq!(expand_braces("no-braces"), vec!["no-braces"]);
    }
}
