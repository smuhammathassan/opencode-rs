//! Local mirrors of the session contracts the runner consumes. `oc-session`
//! is still a stub; these types are promoted there during integration
//! (`TODO(integration): promote to oc-session`).

pub mod event;
pub mod message;
pub mod schema;
pub mod services;
pub mod util;

pub use event::SessionEvent;
pub use message::SessionMessage;
pub use schema::{Location, LocationRef, ModelRef, SessionID, SessionInfo};
