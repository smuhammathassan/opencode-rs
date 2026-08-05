/// From reference/packages/opencode/src/util/wildcard.ts
///
/// Glob-like matching where `*` becomes `.*` and `?` becomes `.` on the
/// (slash-normalized) input. A trailing ` *` makes the tail optional so that
/// `ls *` matches both `ls` and `ls -la`. On Windows the match is
/// case-insensitive, mirroring the `si` regex flags.
use std::collections::HashMap;

fn escape_special(ch: char) -> Option<String> {
    match ch {
        '.' | '+' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
            Some(format!("\\{ch}"))
        }
        _ => None,
    }
}

fn to_regex(pattern: &str, case_insensitive: bool) -> regex::Regex {
    let mut escaped = String::with_capacity(pattern.len() * 2);
    for ch in pattern.chars() {
        match ch {
            '*' => escaped.push_str(".*"),
            '?' => escaped.push('.'),
            _ => match escape_special(ch) {
                Some(out) => escaped.push_str(&out),
                None => escaped.push(ch),
            },
        }
    }
    if escaped.ends_with(" .*") {
        let len = escaped.len() - 3;
        escaped.truncate(len);
        escaped.push_str("( .*)?");
    }
    let flags = if case_insensitive { "(?si)" } else { "(?s)" };
    regex::Regex::new(&format!("{flags}^{escaped}$")).expect("compiled wildcard regex")
}

pub fn match_str(str: &str, pattern: &str) -> bool {
    let normalized = str.replace('\\', "/");
    let pattern = pattern.replace('\\', "/");
    let re = to_regex(&pattern, cfg!(windows));
    re.is_match(&normalized)
}

pub fn all<'a, V>(input: &str, patterns: &'a HashMap<String, V>) -> Option<&'a V> {
    let mut entries: Vec<(&String, &V)> = patterns.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.len().cmp(&b.len()).then(a.cmp(b)));
    let mut result = None;
    for (pattern, value) in entries {
        if match_str(input, pattern) {
            result = Some(value);
        }
    }
    result
}

pub fn all_structured<'a, V>(
    head: &str,
    tail: &[String],
    patterns: &'a HashMap<String, V>,
) -> Option<&'a V> {
    let mut entries: Vec<(&String, &V)> = patterns.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.len().cmp(&b.len()).then(a.cmp(b)));
    let mut result = None;
    for (pattern, value) in entries {
        let parts: Vec<&str> = pattern.split_whitespace().collect();
        if !match_str(head, parts[0]) {
            continue;
        }
        if parts.len() == 1 || match_sequence(tail, &parts[1..]) {
            result = Some(value);
        }
    }
    result
}

fn match_sequence(items: &[String], patterns: &[&str]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let (pattern, rest) = (patterns[0], &patterns[1..]);
    if pattern == "*" {
        return match_sequence(items, rest);
    }
    for (i, item) in items.iter().enumerate() {
        if match_str(item, pattern) && match_sequence(&items[i + 1..], rest) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn matches_literals_and_wildcards() {
        assert!(match_str("hello", "hello"));
        assert!(match_str("hello", "*"));
        assert!(match_str("hello", "hel*"));
        assert!(match_str("hello", "h?llo"));
        assert!(!match_str("hello", "hallo"));
        assert!(!match_str("hello", "hello!"));
    }

    #[test]
    fn matches_multi_segment_with_star() {
        assert!(match_str("ls -la", "ls *"));
        assert!(match_str("ls", "ls *"));
        assert!(match_str("a/b/c", "a/b/*"));
        assert!(match_str("a/b/c", "a/*"));
        assert!(!match_str("a/b/c", "x/*"));
    }

    #[test]
    fn normalizes_backslashes() {
        assert!(match_str("a\\b\\c", "a/b/c"));
    }

    #[test]
    fn escapes_regex_metacharacters() {
        assert!(match_str("file.txt", "file.txt"));
        assert!(match_str("a+b", "a+b"));
        assert!(match_str("(x)", "(x)"));
    }

    #[test]
    fn matches_newlines_with_star() {
        assert!(match_str("a\nb", "a*b"));
    }

    fn patterns() -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        map.insert("npm *".to_string(), json!({ "kind": "npm" }));
        map.insert("git *".to_string(), json!({ "kind": "git" }));
        map.insert("git push *".to_string(), json!({ "kind": "git-push" }));
        map.insert("*".to_string(), json!({ "kind": "any" }));
        map
    }

    #[test]
    fn all_returns_last_matching_pattern() {
        let map = patterns();
        assert_eq!(all("git status", &map).unwrap()["kind"], "git");
        assert_eq!(all("git push origin", &map).unwrap()["kind"], "git-push");
        assert_eq!(all("npm install", &map).unwrap()["kind"], "npm");
        assert_eq!(all("something else", &map).unwrap()["kind"], "any");
    }

    #[test]
    fn all_prefers_longer_patterns() {
        let mut map = HashMap::new();
        map.insert("*".to_string(), "short");
        map.insert("git *".to_string(), "long");
        assert_eq!(all("git status", &map), Some(&"long"));
    }

    #[test]
    fn all_structured_matches_tail_sequence() {
        let mut map = HashMap::new();
        map.insert("deno *".to_string(), "deno");
        map.insert("deno run *".to_string(), "deno-run");
        map.insert("deno run --allow-net *".to_string(), "deno-net");
        map.insert("*".to_string(), "any");
        let tail = vec![
            "run".to_string(),
            "--allow-net".to_string(),
            "server.ts".to_string(),
        ];
        assert_eq!(*all_structured("deno", &tail, &map).unwrap(), "deno-net");
        let tail = vec!["run".to_string(), "server.ts".to_string()];
        assert_eq!(*all_structured("deno", &tail, &map).unwrap(), "deno-run");
        let tail = vec!["lint".to_string()];
        assert_eq!(*all_structured("deno", &tail, &map).unwrap(), "deno");
        assert_eq!(*all_structured("other", &tail, &map).unwrap(), "any");
    }

    #[test]
    fn match_sequence_supports_star_skip() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert!(match_sequence(&items, &["a", "c"]));
        assert!(match_sequence(&items, &["a", "*", "c"]));
        assert!(match_sequence(&items, &["*", "c"]));
        assert!(match_sequence(&items, &["c"]));
        assert!(!match_sequence(&items, &["c", "a"]));
        assert!(match_sequence(&items, &[]));
    }
}
