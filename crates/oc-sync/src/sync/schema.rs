//! Sync event identifiers.
//!
//! From reference/packages/opencode/src/sync/schema.ts:
//!
//! ```ts
//! export const EventID = Schema.String.check(Schema.isStartsWith("evt")).pipe(
//!   Schema.brand("EventID"),
//!   statics((s) => ({
//!     ascending: (id?: string) => s.make(Identifier.ascending("event", id)),
//!   })),
//! )
//! ```
//!
//! The `Identifier` machinery is ported from reference/packages/opencode/src/id/id.ts.

use std::fmt;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Identifiers are 26 chars of base62 after the prefix, time-ordered.
/// From reference/packages/opencode/src/id/id.ts (`LENGTH = 26`).
pub const ID_LENGTH: usize = 26;

const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const TIMESTAMP_BYTES: usize = 6;

/// Known prefixes, matching `prefixes` in reference/packages/opencode/src/id/id.ts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefix {
    Job,
    Event,
    Session,
    Message,
    Permission,
    Question,
    Part,
    Pty,
    Tool,
    Workspace,
}

impl Prefix {
    pub fn as_str(self) -> &'static str {
        match self {
            Prefix::Job => "job",
            Prefix::Event => "evt",
            Prefix::Session => "ses",
            Prefix::Message => "msg",
            Prefix::Permission => "per",
            Prefix::Question => "que",
            Prefix::Part => "prt",
            Prefix::Pty => "pty",
            Prefix::Tool => "tool",
            Prefix::Workspace => "wrk",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierError {
    message: String,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for IdentifierError {}

impl IdentifierError {
    fn invalid(prefix: &str, id: &str) -> Self {
        Self {
            message: format!("ID {id} does not start with {prefix}"),
        }
    }
}

/// Module-global monotonic counter shared across all prefixes and directions,
/// mirroring the module-level `lastTimestamp`/`counter` in
/// reference/packages/opencode/src/id/id.ts.
struct State {
    last_timestamp: u64,
    counter: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    last_timestamp: 0,
    counter: 0,
});

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}

fn next_timestamp(timestamp: u64) -> u64 {
    let mut state = STATE.lock().expect("identifier state poisoned");
    if timestamp != state.last_timestamp {
        state.last_timestamp = timestamp;
        state.counter = 0;
    }
    state.counter += 1;
    state.counter
}

fn random_base62(length: usize) -> String {
    // `crypto.randomBytes` in the reference; uuid v4 provides 16 random bytes.
    let uuid = uuid::Uuid::new_v4();
    let bytes = uuid.as_bytes();
    bytes
        .iter()
        .cycle()
        .take(length)
        .map(|byte| CHARS[*byte as usize % CHARS.len()] as char)
        .collect()
}

/// Create an ID. From `create()` in reference/packages/opencode/src/id/id.ts.
pub fn create(prefix: &str, descending: bool, timestamp: u64) -> String {
    let counter = next_timestamp(timestamp);
    let current = (timestamp as u128) * 0x1000 + counter as u128;
    let value = if descending { !current } else { current };

    let mut time = String::with_capacity(TIMESTAMP_BYTES * 2);
    for i in 0..TIMESTAMP_BYTES {
        let byte = ((value >> (40 - 8 * i)) & 0xff) as u8;
        time.push_str(&format!("{byte:02x}"));
    }

    format!("{prefix}_{time}{}", random_base62(ID_LENGTH - 12))
}

/// Generate an ID for a known prefix, either validating a caller-provided id or
/// minting a new one. From `generateID()` in reference/packages/opencode/src/id/id.ts.
pub fn generate(
    prefix: Prefix,
    descending: bool,
    given: Option<&str>,
) -> Result<String, IdentifierError> {
    let prefix_str = prefix.as_str();
    if let Some(id) = given {
        if !id.starts_with(prefix_str) {
            return Err(IdentifierError::invalid(prefix_str, id));
        }
        return Ok(id.to_string());
    }
    Ok(create(prefix_str, descending, now_ms()))
}

/// `ascending()` from reference/packages/opencode/src/id/id.ts.
pub fn ascending(prefix: Prefix, given: Option<&str>) -> Result<String, IdentifierError> {
    generate(prefix, false, given)
}

/// `descending()` from reference/packages/opencode/src/id/id.ts.
pub fn descending(prefix: Prefix, given: Option<&str>) -> Result<String, IdentifierError> {
    generate(prefix, true, given)
}

/// Extract the millisecond timestamp embedded in an ascending ID.
/// From `timestamp()` in reference/packages/opencode/src/id/id.ts.
pub fn timestamp(id: &str) -> Result<u64, IdentifierError> {
    let prefix = id.split('_').next().unwrap_or_default();
    let start = prefix.len() + 1;
    let end = start + 12;
    let hex = id.get(start..end).ok_or_else(|| IdentifierError {
        message: format!("ID {id} is too short to contain a timestamp"),
    })?;
    let encoded = u128::from_str_radix(hex, 16).map_err(|_| IdentifierError {
        message: format!("ID {id} has an invalid timestamp"),
    })?;
    Ok((encoded / 0x1000) as u64)
}

/// The `EventID` branded string from reference/packages/opencode/src/sync/schema.ts.
///
/// Valid values start with `"evt"` (note: the check is `Schema.isStartsWith("evt")`,
/// no trailing underscore), and `ascending(id)` validates a provided id or creates
/// a new time-ordered one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventID(pub String);

impl EventID {
    /// `EventID.ascending(id?)`. Panics on an invalid provided id to mirror the
    /// reference's `s.make(...)` throwing on invalid input.
    pub fn ascending(given: Option<&str>) -> Result<Self, IdentifierError> {
        ascending(Prefix::Event, given).map(Self)
    }

    /// The `Schema.isStartsWith("evt")` guard from the sync schema.
    pub fn is_valid(value: &str) -> bool {
        value.starts_with("evt")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EventID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<&str> for EventID {
    type Error = IdentifierError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if !Self::is_valid(value) {
            return Err(IdentifierError {
                message: format!("ID {value} does not start with evt"),
            });
        }
        Ok(Self(value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_id_has_prefix_and_length() {
        let id = EventID::ascending(None).unwrap();
        assert!(id.0.starts_with("evt_"), "got {}", id.0);
        // prefix + "_" + ID_LENGTH
        assert_eq!(id.0.len(), 4 + ID_LENGTH);
        assert!(id.0.chars().skip(4).all(|c| CHARS.contains(&(c as u8))));
    }

    #[test]
    fn event_id_timestamp_extraction_round_trips() {
        let id = EventID::ascending(None).unwrap();
        let ts = timestamp(&id.0).unwrap();
        assert!(ts > 0);
        let now = now_ms();
        assert!(now >= ts);
    }

    #[test]
    fn event_id_validates_provided_id() {
        let generated = EventID::ascending(None).unwrap();
        let validated = EventID::ascending(Some(&generated.0)).unwrap();
        assert_eq!(generated, validated);
    }

    #[test]
    fn event_id_rejects_wrong_prefix() {
        let err = EventID::ascending(Some("ses_abc")).unwrap_err();
        assert_eq!(err.message, "ID ses_abc does not start with evt");
    }

    #[test]
    fn event_id_validation_guard() {
        assert!(EventID::is_valid("evt_..."));
        assert!(!EventID::is_valid("ses_..."));
        assert!(EventID::is_valid("evt..."));
    }

    #[test]
    fn ids_are_monotonic() {
        let a = EventID::ascending(None).unwrap();
        let b = EventID::ascending(None).unwrap();
        let ta = timestamp(&a.0).unwrap();
        let tb = timestamp(&b.0).unwrap();
        // Same millisecond window or later; the counter bumps within a window.
        assert!(tb >= ta);
    }

    #[test]
    fn descending_session_id_orders_reverse() {
        let a = descending(Prefix::Session, None).unwrap();
        let b = descending(Prefix::Session, None).unwrap();
        let ta = timestamp(&a).unwrap();
        let tb = timestamp(&b).unwrap();
        // Descending IDs embed ~timestamp, so later creations embed smaller timestamps.
        assert!(tb <= ta);
    }

    #[test]
    fn workspaces_prefix_validation() {
        let ok = ascending(Prefix::Workspace, Some("wrk_x")).unwrap();
        assert_eq!(ok, "wrk_x");
        assert!(ascending(Prefix::Workspace, Some("evt_x")).is_err());
    }

    #[test]
    fn event_id_serializes_as_plain_string() {
        let id = EventID::ascending(None).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", id.0));
    }
}
