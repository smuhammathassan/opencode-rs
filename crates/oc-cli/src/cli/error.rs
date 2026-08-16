//! Error formatting for user-facing output.
//! From reference/packages/opencode/src/cli/error.ts.

use super::effect_cmd::CliError;

/// Mirrors `FormatError(input)`. Returns `None` when the error is not
/// recognised, letting the caller fall back to `FormatUnknownError`.
pub fn format_error(input: &anyhow::Error) -> Option<String> {
    if let Some(cli) = input.downcast_ref::<CliError>() {
        return Some(cli.message.clone());
    }
    for source in input.chain().skip(1) {
        if let Some(cli) = source.downcast_ref::<CliError>() {
            return Some(cli.message.clone());
        }
    }
    None
}

/// Return the explicit exit status carried by a [`CliError`], falling back to
/// the reference command-failure status for ordinary errors.
pub fn exit_code(input: &anyhow::Error) -> i32 {
    if let Some(cli) = input.downcast_ref::<CliError>() {
        return cli.exit_code.unwrap_or(1);
    }
    for source in input.chain().skip(1) {
        if let Some(cli) = source.downcast_ref::<CliError>() {
            return cli.exit_code.unwrap_or(1);
        }
    }
    1
}

/// Mirrors `FormatUnknownError(input)`.
pub fn format_unknown_error(input: &anyhow::Error) -> String {
    if let Some(msg) = format_error(input) {
        return msg;
    }
    // `anyhow`'s alternate display already renders the complete cause chain.
    // Appending `input.chain()` here duplicated every nested cause in the
    // top-level CLI output.
    format!("{:#}", input)
}

#[cfg(test)]
mod tests {
    use super::{exit_code, format_error, format_unknown_error};
    use crate::cli::effect_cmd::{fail, CliError};

    #[test]
    fn preserves_explicit_cli_exit_code() {
        assert_eq!(exit_code(&anyhow::Error::new(fail("cancelled", 2))), 2);
    }

    #[test]
    fn defaults_cli_and_unknown_errors_to_failure() {
        assert_eq!(exit_code(&anyhow::Error::new(CliError::new("failed"))), 1);
        assert_eq!(exit_code(&anyhow::anyhow!("failed")), 1);
    }

    #[test]
    fn formats_cli_errors_without_an_unexpected_error_banner() {
        let error = anyhow::Error::new(CliError::new("File not found: /missing.json"));

        assert_eq!(
            format_error(&error).as_deref(),
            Some("File not found: /missing.json")
        );
        assert_eq!(
            format_unknown_error(&error),
            "File not found: /missing.json"
        );
    }

    #[test]
    fn formats_unknown_error_chain_once() {
        let error = anyhow::anyhow!("root cause").context("outer operation failed");

        assert_eq!(
            format_unknown_error(&error),
            "outer operation failed: root cause"
        );
    }
}
