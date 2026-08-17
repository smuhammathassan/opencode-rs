#![allow(clippy::all)]
//! Agent Client Protocol (ACP) support.
//!
//! A JSON-RPC 2.0 based protocol that lets external AI clients (other agents /
//! IDEs) connect to opencode and drive sessions, tools, permissions, events and
//! usage. This crate mirrors `reference/packages/opencode/src/acp/`.

pub mod agent;
pub mod config_option;
pub mod connection;
pub mod content;
pub mod directory;
pub mod error;
pub mod event;
pub mod jsonrpc;
pub mod permission;
pub mod profile;
pub mod sdk;
pub mod service;
pub mod session;
pub mod tool;
pub mod types;
pub mod usage;
