//! Route machinery: auth, endpoint, framing, protocol, transport, executor,
//! and the `LLMClient` surface.
//! From reference/packages/llm/src/route/*.ts

pub mod auth;
pub mod auth_options;
pub mod client;
pub mod endpoint;
pub mod executor;
pub mod framing;
pub mod protocol;
pub mod transport;

pub use auth::{Auth, AuthInput, Credential, HeaderMap, MissingCredentialError};
pub use client::{compile, LlmClient, Protocol, Route, RouteDefaults, RouteDefaultsInput, RouteMakeInput, RouteModelInput, RoutePatch};
pub use endpoint::{render, Endpoint, EndpointInput, EndpointOptions, EndpointPatch, EndpointPath};
pub use executor::Executor;
pub use framing::Framing;
pub use protocol::{FramePayload, ProtocolStream};