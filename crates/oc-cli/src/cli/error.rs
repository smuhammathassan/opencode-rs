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

/// Mirrors `FormatUnknownError(input)`.
pub fn format_unknown_error(input: &anyhow::Error) -> String {
    if let Some(msg) = format_error(input) {
        return msg;
    }
    let mut out = format!("{:#}", input);
    for source in input.chain().skip(1) {
        out.push('\n');
        out.push_str(&source.to_string());
    }
    out
}
