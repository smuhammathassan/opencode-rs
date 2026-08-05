//! Autocomplete trigger detection and filtering.
//! From reference/packages/tui/src/prompt/display.ts and
//! reference/packages/tui/src/component/prompt/autocomplete.tsx

/// Find the offset of the nearest `@` mention trigger before `offset`, where
/// the text between `@` and the cursor contains no whitespace and the `@` is
/// preceded by whitespace or the start of input.
/// From reference/packages/tui/src/prompt/display.ts (`mentionTriggerIndex`)
pub fn mention_trigger_index(value: &str, offset: usize) -> Option<usize> {
    let chars: Vec<char> = value.chars().collect();
    let offset = offset.min(chars.len());
    let text = &chars[..offset];
    let idx = text.iter().rposition(|&c| c == '@')?;
    let before = if idx == 0 { None } else { Some(text[idx - 1]) };
    let query = &text[idx..];
    if (before.is_none() || before.unwrap().is_whitespace())
        && !query.iter().any(|c| c.is_whitespace())
    {
        Some(idx)
    } else {
        None
    }
}

/// The autocomplete trigger active at the cursor, if any.
/// From reference/packages/tui/src/component/prompt/autocomplete.tsx (`onInput`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Slash,
    Mention,
}

/// Determine which autocomplete popup should be visible for the given input.
pub fn trigger_for_input(value: &str, cursor: usize) -> Option<Trigger> {
    if cursor == 0 {
        return None;
    }
    if value.starts_with('/') {
        let prefix = &value[..cursor.min(value.len())];
        if !prefix[..prefix.len().min(cursor)].contains(' ') && !prefix.contains(' ') {
            return Some(Trigger::Slash);
        }
        if !prefix.contains(' ') {
            return Some(Trigger::Slash);
        }
        return None;
    }
    if mention_trigger_index(value, cursor).is_some() {
        return Some(Trigger::Mention);
    }
    None
}

/// The query text of the current mention (after `@`, before cursor).
pub fn mention_query(value: &str, cursor: usize, trigger_index: usize) -> &str {
    let start = trigger_index + 1;
    let end = cursor.min(value.len()).max(start);
    &value[start.min(value.len())..end]
}

/// Extract a `file#L1-5` line-range suffix from a mention query.
/// From reference/packages/tui/src/component/prompt/autocomplete.tsx
/// (`extractLineRange`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRange {
    pub base_name: String,
    pub start_line: usize,
    pub end_line: Option<usize>,
}

pub fn extract_line_range(input: &str) -> (String, Option<LineRange>) {
    let Some(hash_index) = input.rfind('#') else {
        return (input.to_string(), None);
    };
    let base_name = &input[..hash_index];
    let line_part = &input[hash_index + 1..];
    let re = regex::Regex::new(r"^(\d+)(?:-(\d*))?$").expect("static regex");
    let Some(caps) = re.captures(line_part) else {
        return (base_name.to_string(), None);
    };
    let start_line: usize = caps.get(1).unwrap().as_str().parse().unwrap_or(0);
    let end_line = caps.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
    let end_line = end_line.filter(|e| *e > start_line);
    (
        base_name.to_string(),
        Some(LineRange {
            base_name: base_name.to_string(),
            start_line,
            end_line,
        }),
    )
}

/// Strip the `#L1-5` suffix from a search query.
pub fn remove_line_range(input: &str) -> String {
    let (base, _) = extract_line_range(input);
    base
}

/// Simple fuzzy subsequence scorer with contiguous bonus; used in place of the
/// reference's fuzzysort. Returns a higher score for better matches.
pub fn fuzzy_score(query: &str, target: &str) -> Option<f32> {
    let query: Vec<char> = query.trim().chars().collect();
    if query.is_empty() {
        return Some(0.0);
    }
    let target: Vec<char> = target.trim().chars().collect();
    if query.len() > target.len() {
        return None;
    }
    let mut qi = 0usize;
    let mut contiguous = 0usize;
    let mut weight = 0f32;
    for &c in target.iter() {
        if qi < query.len() && c.eq_ignore_ascii_case(&query[qi]) {
            contiguous += 1;
            weight += 1.0 + (contiguous - 1) as f32 * 0.75;
            qi += 1;
        } else {
            contiguous = 0;
        }
    }
    if qi < query.len() {
        return None;
    }
    let prefix_bonus = if target[..query.len()]
        .iter()
        .zip(&query)
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
    {
        0.6
    } else {
        0.0
    };
    Some(weight / target.len() as f32 + prefix_bonus)
}

/// Filter and rank options by fuzzy relevance.
pub fn fuzzy_filter<'a, T>(
    query: &str,
    options: &'a [T],
    key: impl Fn(&T) -> String,
) -> Vec<(f32, &'a T)> {
    let mut scored: Vec<(f32, &T)> = options
        .iter()
        .filter_map(|option| {
            let target = key(option);
            fuzzy_score(query, &target).map(|score| (score, option))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_trigger_detection() {
        assert_eq!(mention_trigger_index("hello @wor", 10), Some(6));
        assert_eq!(mention_trigger_index("hello @wor ld", 8), Some(6));
        // The query between @ and the cursor still has no whitespace.
        assert_eq!(mention_trigger_index("hello @wor ld", 10), Some(6));
        // Whitespace inside the query disables the trigger.
        assert_eq!(mention_trigger_index("hello @wor ld", 12), None);
        // @ mid-word is not a trigger.
        assert_eq!(mention_trigger_index("em@il", 4), None);
        // Leading @ is a trigger.
        assert_eq!(mention_trigger_index("@agent", 6), Some(0));
    }

    #[test]
    fn slash_trigger_only_at_start() {
        assert_eq!(trigger_for_input("/models", 7), Some(Trigger::Slash));
        assert_eq!(trigger_for_input("/models foo", 11), None);
        assert_eq!(trigger_for_input("not /slash", 10), None);
        assert_eq!(trigger_for_input("", 0), None);
    }

    #[test]
    fn mention_trigger_wraps() {
        assert_eq!(trigger_for_input("look @fi", 8), Some(Trigger::Mention));
        assert_eq!(mention_query("look @fi", 8, 5), "fi");
    }

    #[test]
    fn line_range_extraction() {
        assert_eq!(
            extract_line_range("src/a.ts"),
            ("src/a.ts".to_string(), None)
        );
        assert_eq!(
            extract_line_range("src/a.ts#5"),
            (
                "src/a.ts".to_string(),
                Some(LineRange {
                    base_name: "src/a.ts".to_string(),
                    start_line: 5,
                    end_line: None,
                })
            )
        );
        assert_eq!(
            extract_line_range("src/a.ts#5-10"),
            (
                "src/a.ts".to_string(),
                Some(LineRange {
                    base_name: "src/a.ts".to_string(),
                    start_line: 5,
                    end_line: Some(10),
                })
            )
        );
        // Invalid suffix falls back to base.
        assert_eq!(
            extract_line_range("src/a.ts#abc"),
            ("src/a.ts".to_string(), None)
        );
        assert_eq!(remove_line_range("src/a.ts#5-10"), "src/a.ts");
    }

    #[test]
    fn fuzzy_matching() {
        assert!(fuzzy_score("mod", "model").is_some());
        assert!(!fuzzy_score("models", "model").is_some());
        assert_eq!(fuzzy_score("", "anything"), Some(0.0));
        // Exact prefix scores higher than a scattered match.
        let a = fuzzy_score("mo", "model").unwrap();
        let b = fuzzy_score("mo", "compact o m").unwrap();
        assert!(a > b);
        assert!(fuzzy_score("xyz", "no match").is_none());
    }

    #[test]
    fn fuzzy_filter_ranks() {
        let options = vec!["model", "models", "move", "modes"];
        let ranked = fuzzy_filter("mod", &options, |s| s.to_string());
        assert_eq!(ranked[0].1, &"model");
        assert_eq!(ranked[1].1, &"modes");
        assert_eq!(ranked[2].1, &"models");
        assert!(!ranked.iter().any(|(_, o)| **o == "move"));
        let missing = fuzzy_filter("zzz", &options, |s| s.to_string());
        assert!(missing.is_empty());
    }
}
