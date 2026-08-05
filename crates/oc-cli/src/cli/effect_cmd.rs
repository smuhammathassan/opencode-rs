//! User-visible command failure helper.
//! From reference/packages/opencode/src/cli/effect-cmd.ts.

use std::fmt;

/// A user-visible command failure. Throwing this from a handler surfaces a
/// printed message plus a non-zero exit, recognised by the global error
/// formatter in `error.rs`.
#[derive(Debug, Clone)]
pub struct CliError {
    pub message: String,
    pub exit_code: Option<i32>,
}

impl CliError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: None,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CliError {}

/// Mirrors `fail(message, exitCode?)` from effect-cmd.ts.
pub fn fail(message: impl Into<String>, exit_code: i32) -> CliError {
    CliError {
        message: message.into(),
        exit_code: Some(exit_code),
    }
}

/// Build an `anyhow::Error` carrying a `CliError` so the top-level formatter
/// prints a clean message instead of the "Unexpected error" banner.
pub fn not_wired(what: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CliError::new(what))
}
