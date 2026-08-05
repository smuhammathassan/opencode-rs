//! From reference/packages/schema/src/session-id.ts

use crate::identifier::descending as identifier_descending;

/// `Schema.String.check(Schema.isStartsWith("ses")).pipe(Schema.brand("SessionID"))`.
pub type SessionID = String;

/// `SessionID.descending(id?)` — creates a new `ses_` ID or validates the given one.
pub fn descending(id: Option<String>) -> Result<SessionID, SessionIDError> {
    match id {
        None => Ok(create()),
        Some(id) if id.starts_with("ses") => Ok(id),
        Some(id) => Err(SessionIDError::InvalidPrefix(id)),
    }
}

/// `SessionID.create()`.
pub fn create() -> SessionID {
    format!("ses_{}", identifier_descending())
}

/// Error type mirroring the thrown error in `SessionID.descending`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SessionIDError {
    #[error("ID {0} does not start with ses")]
    InvalidPrefix(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_ses_ids() {
        let id = create();
        assert!(id.starts_with("ses_"));
        assert_eq!(id.len(), 30);
    }
}
