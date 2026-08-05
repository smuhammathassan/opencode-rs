//! Wire data types for the client.
//!
//! Mirrors the schema DTOs used by `reference/packages/client/src/generated/types.ts`.
//! These are local mirrors until `oc-schema` is populated.
// TODO(integration): promote to oc-schema once the schema crate is implemented.

pub mod agent;
pub mod command;
pub mod connection;
pub mod credential;
pub mod event;
pub mod filesystem;
pub mod health;
pub mod integration;
pub mod location;
pub mod model;
pub mod permission;
pub mod permission_saved;
pub mod project;
pub mod project_copy;
pub mod prompt;
pub mod provider;
pub mod pty;
pub mod question;
pub mod reference;
pub mod revert;
pub mod schema;
pub mod session;
pub mod session_input;
pub mod session_message;
pub mod skill;

pub use agent::*;
pub use command::*;
pub use connection::*;
pub use credential::*;
pub use event::*;
pub use filesystem::*;
pub use health::*;
pub use integration::*;
pub use location::*;
pub use model::*;
pub use permission::*;
pub use permission_saved::*;
pub use project::*;
pub use project_copy::*;
pub use prompt::*;
pub use provider::*;
pub use pty::*;
pub use question::*;
pub use reference::*;
pub use revert::*;
pub use schema::*;
pub use session::*;
pub use session_input::*;
pub use session_message::*;
pub use skill::*;
