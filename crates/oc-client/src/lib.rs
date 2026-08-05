//! Typed RPC client for the opencode server API.
//!
//! A 1:1 Rust port of the promise client in
//! `reference/packages/client/src/generated/client.ts` (and its typed inputs and
//! outputs from `generated/types.ts`), backed by `reqwest`. The protocol errors,
//! group/endpoint contract, and `_tag`-discriminated error bodies mirror
//! `reference/packages/protocol/src/errors.ts`, `protocol/src/groups/*`, and
//! `protocol/src/middleware/*`. The public surface follows the SDK shape in
//! `reference/packages/sdk/js/src/client.ts`.
//!
//! ```
//! use oc_client::{ClientOptions, OpenCode};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = OpenCode::make(ClientOptions {
//!     base_url: "http://localhost:3000".parse()?,
//!     ..ClientOptions::default()
//! })?;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod contract;
pub mod effect;
pub mod error;
pub mod generated;
pub mod middleware;
pub mod sse;
pub mod transport;
pub mod types;

pub use generated::*;
