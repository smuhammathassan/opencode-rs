/// From reference/packages/opencode/src/permission/index.ts and
/// reference/packages/core/src/util/wildcard.ts — pure permission evaluation
/// helpers used by the system prompt and tool resolution.
///
/// TODO(integration): promote to oc-core (Wildcard) / oc-schema (ruleset).
use crate::v1::{PermissionRule, Ruleset};

pub mod wildcard {
    /// From reference `core/src/util/wildcard.ts:match`.
    pub fn matches(input: &str, pattern: &str) -> bool {
        let normalized = input.replace('\\', "/");
        let mut escaped = String::with_capacity(pattern.len());
        for c in pattern.replace('\\', "/").chars() {
            match c {
                '.' | '+' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
                    escaped.push('\\');
                    escaped.push(c);
                }
                '?' => escaped.push('.'),
                '*' => escaped.push_str(".*"),
                other => escaped.push(other),
            }
        }

        if escaped.ends_with(" .*") {
            let len = escaped.len();
            escaped = format!("{}( .*)?", &escaped[..len - 3]);
        }

        let anchored = format!("^{escaped}$");
        match regex::Regex::new(&anchored) {
            Ok(regex) => regex.is_match(&normalized),
            Err(_) => false,
        }
    }
}

/// From reference `permission/index.ts:merge`.
pub fn merge<'a>(rulesets: impl IntoIterator<Item = &'a Ruleset>) -> Ruleset {
    rulesets
        .into_iter()
        .flat_map(|ruleset| ruleset.clone())
        .collect()
}

/// From reference `permission/index.ts:evaluate`.
pub fn evaluate(permission: &str, pattern: &str, rulesets: &[&Ruleset]) -> PermissionRule {
    rulesets
        .iter()
        .flat_map(|ruleset| ruleset.iter())
        .rev()
        .find(|rule| {
            wildcard::matches(permission, &rule.permission)
                && wildcard::matches(pattern, &rule.pattern)
        })
        .cloned()
        .unwrap_or(PermissionRule {
            action: "ask".to_string(),
            permission: permission.to_string(),
            pattern: "*".to_string(),
        })
}

/// From reference `permission/index.ts:disabled`.
pub fn disabled(tools: &[String], ruleset: &Ruleset) -> std::collections::HashSet<String> {
    let edits = ["edit", "write", "apply_patch"];
    let reads = [
        "list_mcp_resources",
        "list_mcp_resource_templates",
        "read_mcp_resource",
    ];
    tools
        .iter()
        .filter(|tool| {
            let permission = if edits.contains(&tool.as_str()) {
                "edit"
            } else if reads.contains(&tool.as_str()) {
                "read"
            } else {
                tool.as_str()
            };
            let rule = ruleset
                .iter()
                .rev()
                .find(|rule| wildcard::matches(permission, &rule.permission));
            matches!(rule, Some(rule) if rule.pattern == "*" && rule.action == "deny")
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(permission: &str, pattern: &str, action: &str) -> PermissionRule {
        PermissionRule {
            permission: permission.to_string(),
            pattern: pattern.to_string(),
            action: action.to_string(),
        }
    }

    #[test]
    fn wildcard_star_matches_any() {
        assert!(wildcard::matches("read", "*"));
        assert!(wildcard::matches("skill", "*"));
        assert!(wildcard::matches("mcp:server:uri", "*"));
    }

    #[test]
    fn disabled_skips_non_wildcard_patterns() {
        let ruleset = vec![rule("edit", "*", "deny")];
        let disabled = disabled(&["edit".to_string()], &ruleset);
        assert!(disabled.contains("edit"));
    }

    #[test]
    fn evaluate_falls_back_to_ask() {
        let result = evaluate("bash", "*", &[]);
        assert_eq!(result.action, "ask");
    }
}
