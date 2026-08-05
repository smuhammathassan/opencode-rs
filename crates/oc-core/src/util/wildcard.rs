//! Wildcard pattern matching.
//! From reference/packages/core/src/util/wildcard.ts

use regex::Regex;

/// Mirrors `Wildcard.match(input, pattern)` — `*` and `?` wildcards with
/// backslash normalization. The reference compiles to `^pattern$` (dotall; the
/// `s` flag). Case-insensitivity applies only on Windows.
pub fn wildcard_match(input: &str, pattern: &str) -> bool {
    let normalized = input.replace('\\', "/");
    let mut escaped = pattern.replace('\\', "/");
    escaped = escape_regex(&escaped);
    escaped = escaped.replace('*', ".*").replace('?', ".");
    if let Some(stripped) = escaped.strip_suffix(" .*") {
        escaped = format!("{stripped}( .*)?");
    }
    let expression = format!("^{escaped}$");
    Regex::new(&expression)
        .map(|re| re.is_match(&normalized))
        .unwrap_or(false)
}

fn escape_regex(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '.' | '+' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_wildcards() {
        assert!(wildcard_match("provider.use", "provider.*"));
        assert!(wildcard_match("a.b.c", "*"));
        assert!(!wildcard_match("x", "y"));
    }

    #[test]
    fn question_mark() {
        assert!(wildcard_match("abc", "a?c"));
        assert!(!wildcard_match("abbc", "a?c"));
    }

    #[test]
    fn escapes_regex_chars() {
        assert!(wildcard_match("a.b", "a.b"));
        assert!(!wildcard_match("axb", "a.b"));
        assert!(wildcard_match("a.b", "a.b"));
    }

    #[test]
    fn trailing_space_star_optional() {
        // "x *" compiles to "x( .*)?" — matches "x" and "x <rest>".
        assert!(wildcard_match("dir", "dir *"));
        assert!(wildcard_match("dir anything", "dir *"));
        assert!(!wildcard_match("dir/anything", "dir *"));
        assert!(!wildcard_match("other", "dir *"));
    }
}
