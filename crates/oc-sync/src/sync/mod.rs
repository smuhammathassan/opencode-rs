//! Sync: event-sourcing of session events with total ordering by cursor.
//!
//! From reference/packages/opencode/src/sync/ — see README.md for the design:
//! one writer per session aggregate, a per-aggregate monotonically increasing
//! `seq`, events emitted *before* the mutation, and replay of a remote event log
//! to synchronize other devices. `schema.ts` only defines `EventID` in
//! v1.18.13; the actual runtime is `EventV2` in reference/packages/core/src/event.ts
//! which this module mirrors.

pub mod event;
pub mod schema;
pub mod sql;
pub mod store;
