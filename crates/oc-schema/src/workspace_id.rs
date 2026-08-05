//! From reference/packages/schema/src/workspace-id.ts

use crate::identifier::ascending as identifier_ascending;

/// `Schema.String.check(Schema.isStartsWith("wrk")).pipe(Schema.brand("WorkspaceV2.ID"))`.
pub type WorkspaceID = String;

/// `WorkspaceID.ascending(id?)` — creates a new `wrk_` ID or validates the given one.
pub fn ascending(id: Option<String>) -> Result<WorkspaceID, WorkspaceIDError> {
    match id {
        None => Ok(create()),
        Some(id) if id.starts_with("wrk") => Ok(id),
        Some(id) => Err(WorkspaceIDError::InvalidPrefix(id)),
    }
}

/// `WorkspaceID.create()`.
pub fn create() -> WorkspaceID {
    format!("wrk_{}", identifier_ascending())
}

/// Error type mirroring the thrown error in `WorkspaceID.ascending`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WorkspaceIDError {
    #[error("ID {0} does not start with wrk")]
    InvalidPrefix(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_wrk_ids() {
        let id = create();
        assert!(id.starts_with("wrk_"));
        assert_eq!(id.len(), 30);
    }

    #[test]
    fn accepts_or_rejects_given_ids() {
        assert_eq!(ascending(Some("wrk_x".to_string())).unwrap(), "wrk_x");
        assert!(ascending(Some("ses_x".to_string())).is_err());
        assert!(ascending(None).unwrap().starts_with("wrk_"));
    }
}
