//! Small host utilities shared across tool leaves.

/// BOM helpers from `reference/packages/opencode/src/util/bom.ts`.
pub mod bom {
    /// `Bom.split` from `reference/packages/opencode/src/util/bom.ts:4`.
    pub fn split(text: &str) -> (bool, String) {
        if text.starts_with('\u{feff}') {
            (true, text[3..].to_string())
        } else {
            (false, text.to_string())
        }
    }

    /// `Bom.join` from `reference/packages/opencode/src/util/bom.ts:12`.
    pub fn join(text: &str, bom: bool) -> String {
        let stripped = split(text).1;
        if !bom {
            return stripped;
        }
        format!("\u{feff}{stripped}")
    }

    /// Read a file, splitting any leading UTF-8 BOM.
    pub fn read_file(file_path: &str) -> std::io::Result<(bool, String)> {
        let bytes = std::fs::read(file_path)?;
        let (bom, text) = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            (true, String::from_utf8_lossy(&bytes[3..]).to_string())
        } else {
            (false, String::from_utf8_lossy(&bytes).to_string())
        };
        Ok((bom, text))
    }
}

/// `FSUtil.contains` from `reference/packages/core/src/fs-util.ts:270`.
pub fn fs_contains(parent: &str, child: &str) -> bool {
    let parent_abs =
        std::path::absolute(parent).unwrap_or_else(|_| std::path::PathBuf::from(parent));
    let child_abs = std::path::absolute(child).unwrap_or_else(|_| std::path::PathBuf::from(child));
    match child_abs.strip_prefix(&parent_abs) {
        Ok(rest) => rest.as_os_str().is_empty() || !rest.starts_with(".."),
        Err(_) => false,
    }
}

/// `path.relative` — best-effort mirror of Node's `path.relative`.
pub fn path_relative(parent: &str, child: &str) -> String {
    let parent_abs =
        std::path::absolute(parent).unwrap_or_else(|_| std::path::PathBuf::from(parent));
    let child_abs = std::path::absolute(child).unwrap_or_else(|_| std::path::PathBuf::from(child));
    if parent_abs == child_abs {
        return String::new();
    }
    match child_abs.strip_prefix(&parent_abs) {
        Ok(rest) => rest.to_string_lossy().to_string(),
        Err(_) => child.to_string(),
    }
}

/// `path.resolve` — absolute path resolution like Node's `path.resolve`.
pub fn path_resolve(root: &str, path: &str) -> String {
    let joined = std::path::Path::new(root).join(path);
    std::path::absolute(&joined)
        .unwrap_or(joined)
        .to_string_lossy()
        .to_string()
}

/// `Wildcard.match` from `reference/packages/core/src/util/wildcard.ts`.
pub fn wildcard_match(input: &str, pattern: &str) -> bool {
    let normalized = input.replace('\\', "/");
    let mut escaped = pattern
        .replace('\\', "/")
        .replace(
            ['.', '+', '^', '$', '{', '}', '(', ')', '|', '[', ']', '\\'],
            "\\$0",
        )
        .replace('*', ".*")
        .replace('?', ".");
    if escaped.ends_with(" .*") {
        let len = escaped.len();
        escaped = format!("{}( .*)?", &escaped[..len - 3]);
    }
    let expression = format!("^{escaped}$");
    regex::Regex::new(&expression)
        .map(|re| re.is_match(&normalized))
        .unwrap_or(false)
}

/// `Identifier` from `reference/packages/opencode/src/id/id.ts`.
pub mod identifier {
    use std::sync::atomic::{AtomicU64, Ordering};

    static LAST_TIMESTAMP: AtomicU64 = AtomicU64::new(0);
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    const LENGTH: usize = 26;
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    /// `Identifier.create` from `reference/packages/opencode/src/id/id.ts:51`.
    pub fn create(prefix: &str, direction: &str, timestamp: Option<u64>) -> String {
        let current = timestamp.unwrap_or_else(now_millis);
        let mut counter = COUNTER.load(Ordering::SeqCst);
        let last = LAST_TIMESTAMP.load(Ordering::SeqCst);
        if current != last {
            LAST_TIMESTAMP.store(current, Ordering::SeqCst);
            counter = 0;
            COUNTER.store(0, Ordering::SeqCst);
        } else {
            counter += 1;
            COUNTER.store(counter, Ordering::SeqCst);
        }

        let mut now: u128 = u128::from(current) * 0x1000 + u128::from(counter);
        if direction == "descending" {
            now = !now;
        }
        let mut time_bytes = [0u8; 6];
        for i in 0..6 {
            time_bytes[i] = ((now >> (40 - 8 * i)) & 0xff) as u8;
        }
        let mut out = String::new();
        for byte in time_bytes {
            out.push(HEX_CHARS[(byte >> 4) as usize] as char);
            out.push(HEX_CHARS[(byte & 0xf) as usize] as char);
        }
        out.push_str(&random_base62(LENGTH - 12));
        format!("{prefix}_{out}")
    }

    pub fn ascending(prefix: &str) -> String {
        create(prefix, "ascending", None)
    }

    /// `Identifier.timestamp` from `reference/packages/opencode/src/id/id.ts:73`.
    pub fn timestamp(id: &str) -> u64 {
        let prefix = id.split('_').next().unwrap_or("");
        let start = prefix.len() + 1;
        let end = start + 12;
        let hex = &id[start..end.min(id.len())];
        let encoded = u128::from_str_radix(hex, 16).unwrap_or(0);
        (encoded / 0x1000) as u64
    }

    fn now_millis() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn random_base62(length: usize) -> String {
        let mut seed = [0u8; 16];
        // std-only pseudo-random seeding; collision resistance is not required.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        seed[0] = (nanos & 0xff) as u8;
        seed[1] = ((nanos >> 8) & 0xff) as u8;
        seed[2] = ((nanos >> 16) & 0xff) as u8;
        seed[3] = ((nanos >> 24) & 0xff) as u8;
        for (i, byte) in seed[4..].iter_mut().enumerate() {
            *byte = ((std::process::id() as usize + i).wrapping_mul(2654435761)) as u8;
        }
        let mut out = String::new();
        for i in 0..length {
            out.push(BASE62[seed[i % seed.len()] as usize % 62] as char);
        }
        out
    }
}

/// `Permission.evaluate` from `reference/packages/opencode/src/permission/index.ts:28`,
/// using `PermissionV1.Rule` (`reference/packages/schema/src/permission.ts`).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub permission: String,
    pub pattern: String,
    pub action: String,
}

pub fn evaluate(permission: &str, pattern: &str, rulesets: &[&[Rule]]) -> Rule {
    for ruleset in rulesets.iter().rev() {
        for rule in ruleset.iter().rev() {
            if wildcard_match(permission, &rule.permission)
                && wildcard_match(pattern, &rule.pattern)
            {
                return rule.clone();
            }
        }
    }
    Rule {
        permission: permission.to_string(),
        pattern: "*".to_string(),
        action: "ask".to_string(),
    }
}

/// The current calendar year (used by the websearch description template).
pub fn current_year() -> i32 {
    use chrono::Datelike;
    chrono::Local::now().year()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches() {
        assert!(wildcard_match("bash", "bash"));
        assert!(wildcard_match("bash", "*"));
        assert!(wildcard_match("read", "read"));
        assert!(wildcard_match("edit", "edit"));
        assert!(!wildcard_match("read", "bash"));
        assert!(wildcard_match("tool.write", "*"));
    }

    #[test]
    fn identifier_roundtrip_timestamp() {
        // NOTE: like the reference `Identifier.create`, only timestamps whose
        // 48-bit encoding does not overflow survive a round trip; pick a small
        // value. Larger ms values truncate identically in both implementations.
        let id = identifier::create("tool", "ascending", Some(1_234_567));
        assert!(id.starts_with("tool_"));
        assert_eq!(identifier::timestamp(&id), 1_234_567);
    }

    #[test]
    fn evaluate_respects_last_rule() {
        let allow = Rule {
            permission: "task".into(),
            pattern: "*".into(),
            action: "allow".into(),
        };
        let deny = Rule {
            permission: "task".into(),
            pattern: "*".into(),
            action: "deny".into(),
        };
        let result = evaluate("task", "*", &[&[allow, deny]]);
        assert_eq!(result.action, "deny");
    }
}
