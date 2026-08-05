//! Domain ID generators with reference prefixes.
//!
//! From reference/packages/core/src/id/id.ts.

use crate::identifier;

/// The prefix table from the reference.
pub const PREFIXES: [(&str, &str); 10] = [
    ("job", "job"),
    ("event", "evt"),
    ("session", "ses"),
    ("message", "msg"),
    ("permission", "per"),
    ("question", "que"),
    ("part", "prt"),
    ("pty", "pty"),
    ("tool", "tool"),
    ("workspace", "wrk"),
];

/// Sort direction for a generated ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

impl Direction {
    fn descending(self) -> bool {
        matches!(self, Direction::Descending)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("unknown ID prefix key `{0}`")]
    UnknownPrefix(String),
    #[error("ID `{given}` does not start with `{expected}`")]
    InvalidPrefix { given: String, expected: String },
}

/// Mirrors `Identifier.ascending(prefix, given?)`.
pub fn ascending(prefix: &str, given: Option<&str>) -> Result<String, IdError> {
    generate(prefix, Direction::Ascending, given)
}

/// Mirrors `Identifier.descending(prefix, given?)`.
pub fn descending(prefix: &str, given: Option<&str>) -> Result<String, IdError> {
    generate(prefix, Direction::Descending, given)
}

fn generate(prefix: &str, direction: Direction, given: Option<&str>) -> Result<String, IdError> {
    let short = PREFIXES
        .iter()
        .find(|(key, _)| *key == prefix)
        .map(|(_, value)| *value)
        .ok_or_else(|| IdError::UnknownPrefix(prefix.to_string()))?;
    match given {
        None => Ok(create(short, direction, None)),
        Some(value) => {
            if !value.starts_with(short) {
                return Err(IdError::InvalidPrefix {
                    given: value.to_string(),
                    expected: short.to_string(),
                });
            }
            Ok(value.to_string())
        }
    }
}

/// Mirrors `Identifier.create(prefix, direction, timestamp?)`.
pub fn create(prefix: &str, direction: Direction, timestamp: Option<u64>) -> String {
    let stamp = timestamp.unwrap_or_else(identifier::now_ms);
    format!(
        "{prefix}_{}",
        identifier::create(direction.descending(), stamp)
    )
}

/// Extract timestamp from an ascending ID. Does not work with descending IDs.
/// Mirrors `Identifier.timestamp(id)`.
pub fn timestamp(id: &str) -> u64 {
    let prefix_len = id.split('_').next().map_or(0, |p| p.len());
    if id.len() < prefix_len + 13 {
        return 0;
    }
    let hex = &id[prefix_len + 1..prefix_len + 13];
    let encoded = u64::from_str_radix(hex, 16).unwrap_or(0);
    encoded / 0x1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascending_with_given_prefix_passes_through() {
        assert_eq!(ascending("event", Some("evt_123")).unwrap(), "evt_123");
    }

    #[test]
    fn ascending_rejects_bad_prefix() {
        let err = ascending("event", Some("ses_123")).unwrap_err();
        assert_eq!(
            err,
            IdError::InvalidPrefix {
                given: "ses_123".to_string(),
                expected: "evt".to_string()
            }
        );
    }

    #[test]
    fn unknown_prefix_key() {
        assert_eq!(
            ascending("nope", None).unwrap_err(),
            IdError::UnknownPrefix("nope".to_string())
        );
    }

    #[test]
    fn generated_id_has_prefix() {
        for (key, short) in PREFIXES {
            let id = ascending(key, None).unwrap();
            assert!(id.starts_with(short), "{key} -> {id}");
        }
    }

    #[test]
    fn timestamp_roundtrip() {
        // Small timestamps roundtrip exactly.
        let stamp = 100000u64;
        let id = create("evt", Direction::Ascending, Some(stamp));
        assert!(id.starts_with("evt_"));
        assert_eq!(timestamp(&id), stamp);
    }

    #[test]
    fn timestamp_wraps_at_48_bits_like_reference() {
        // The 12-hex time field holds `timestamp * 0x1000` mod 2^48, so
        // timestamps >= 2^36 ms wrap, exactly like the reference.
        let stamp = 1730000000000u64;
        let id = create("evt", Direction::Ascending, Some(stamp));
        let expected = stamp % (1u64 << 36);
        assert_eq!(timestamp(&id), expected);
    }
}
