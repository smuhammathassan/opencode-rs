// Minimal glob supporting the brace/star patterns the config loader uses.
//
// From reference/packages/core/src/util/glob.ts (patterns used by the config
// loader: `{agent,agents}/**/*.md`, `{mode,modes}/*.md`,
// `{command,commands}/**/*.md`, `{plugin,plugins}/*.{ts,js}`).

use std::path::{Path, PathBuf};

/// Expands one path segment containing `{a,b}` groups into alternatives.
fn expand_braces(segment: &str) -> Vec<String> {
    match segment.find('{') {
        None => vec![segment.to_string()],
        Some(start) => {
            let Some(end) = segment[start + 1..].find('}').map(|i| i + start + 1) else {
                return vec![segment.to_string()];
            };
            let options: Vec<&str> = segment[start + 1..end].split(',').collect();
            if options.is_empty() {
                return vec![segment.to_string()];
            }
            let prefix = &segment[..start];
            let suffix = &segment[end + 1..];
            let mut out = Vec::new();
            for option in options {
                for rest in expand_braces(suffix) {
                    out.push(format!("{prefix}{option}{rest}"));
                }
            }
            out
        }
    }
}

/// Matches a single path segment against a `*` pattern (ASCII-aware).
fn segment_matches(name: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return name == pattern;
    }
    let bytes = name.as_bytes();
    let pattern_bytes = pattern.as_bytes();
    let (n, m) = (bytes.len(), pattern_bytes.len());
    let mut dp = vec![vec![false; m + 1]; n + 1];
    dp[0][0] = true;
    for j in 1..=m {
        if pattern_bytes[j - 1] == b'*' {
            dp[0][j] = dp[0][j - 1];
        }
    }
    for i in 1..=n {
        for j in 1..=m {
            dp[i][j] = if pattern_bytes[j - 1] == b'*' {
                dp[i - 1][j] || dp[i][j - 1]
            } else {
                dp[i - 1][j - 1] && bytes[i - 1] == pattern_bytes[j - 1]
            };
        }
    }
    dp[n][m]
}

/// Scans `cwd` for files matching `pattern`. Returns absolute paths sorted by
/// name, like `Glob.scan(..., { absolute: true })`.
pub fn scan(pattern: &str, cwd: &Path) -> Vec<PathBuf> {
    let segments: Vec<Vec<String>> = pattern.split('/').map(expand_braces).collect();
    let mut out = Vec::new();
    walk(cwd, &segments, 0, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, segments: &[Vec<String>], index: usize, out: &mut Vec<PathBuf>) {
    if index == segments.len() {
        out.push(dir.to_path_buf());
        return;
    }
    for alternative in &segments[index] {
        if alternative == "**" {
            walk(dir, segments, index + 1, out);
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    walk(&entry.path(), segments, index, out);
                }
            }
        } else {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut matches = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if segment_matches(&name, alternative) {
                    matches.push(path);
                }
            }
            matches.sort();
            for path in matches {
                walk(&path, segments, index + 1, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{expand_braces, segment_matches};

    #[test]
    fn matches_segments() {
        assert!(segment_matches("foo.md", "*.md"));
        assert!(segment_matches("foo.ts", "*.ts"));
        assert!(segment_matches("foo.js", "*.js"));
        assert!(!segment_matches("foo.txt", "*.ts"));
        assert!(!segment_matches(".hidden", "*.md"));
        assert!(segment_matches("a-b-c", "a-*"));
        assert!(segment_matches("agent", "agent"));
        assert!(!segment_matches(".agent", "agent"));
    }

    #[test]
    fn expands_braces() {
        assert_eq!(expand_braces("{agent,agents}"), vec!["agent", "agents"]);
        assert_eq!(expand_braces("*.{ts,js}"), vec!["*.ts", "*.js"]);
        assert_eq!(expand_braces("plain"), vec!["plain"]);
    }
}
