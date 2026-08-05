/// From reference/packages/opencode/src/session/schema.ts
///
/// V1 session identifiers used by the opencode session service. These mirror
/// the branded effect schemas: `SessionID` = `ses_` + descending(),
/// `MessageID` = `msg` + ascending(), `PartID` = `prt` + ascending().
use crate::identifier;

pub const SESSION_PREFIX: &str = "ses";
pub const MESSAGE_PREFIX: &str = "msg";
pub const PART_PREFIX: &str = "prt";

pub fn create_session(id: Option<&str>) -> String {
    identifier::descending(SESSION_PREFIX, id).expect("session id prefix is valid")
}

pub fn create_message(id: Option<&str>) -> String {
    identifier::ascending(MESSAGE_PREFIX, id).expect("message id prefix is valid")
}

pub fn create_part(id: Option<&str>) -> String {
    identifier::ascending(PART_PREFIX, id).expect("part id prefix is valid")
}

pub fn is_session(id: &str) -> bool {
    id.starts_with("ses_")
}

pub fn is_message(id: &str) -> bool {
    id.starts_with("msg")
}

pub fn is_part(id: &str) -> bool {
    id.starts_with("prt")
}
