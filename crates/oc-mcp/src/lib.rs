//! oc-mcp — a Model Context Protocol client, ported from
//! `reference/packages/opencode/src/mcp/` (opencode v1.18.13).
//!
//! Provides a stdio transport for local MCP servers (`command: [...]` config)
//! and HTTP/SSE transports for remote servers (`url:` config), with the
//! `initialize` handshake, `tools/list`, `tools/call`, resources, prompts, and
//! OAuth flows for remote servers.

pub mod auth;
pub mod browser;
pub mod catalog;
pub mod client;
pub mod config;
pub mod crypto;
pub mod index;
pub mod jsonrpc;
pub mod oauth_callback;
pub mod oauth_provider;
pub mod transport;
pub mod types;
pub mod util;

/// OAuth client flow (port of `@modelcontextprotocol/sdk` `client/auth.js`).
pub mod oauth;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error("Operation timed out after {ms}ms ({label})")]
    Timeout { ms: u64, label: String },
    #[error("MCP server returned error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("Unauthorized: {message}")]
    Unauthorized { message: String },
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error("OAuth: {0}")]
    OAuth(String),
}

impl Error {
    pub fn message(message: impl Into<String>) -> Self {
        Error::Message(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Error::Unauthorized {
            message: message.into(),
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::Unauthorized { .. })
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
