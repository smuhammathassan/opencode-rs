//! `opencode completion`
//! Mirrors the yargs `.completion("completion", "generate shell completion script")`.

use crate::cli::ui;

pub fn run() -> anyhow::Result<i32> {
    // TODO(integration): emit bash/zsh/fish completion via `clap_complete`.
    ui::println(&[
        crate::cli::ui::Style::TEXT_WARNING_BOLD,
        "!  ",
        crate::cli::ui::Style::TEXT_NORMAL,
        "shell completion generation is not yet wired in this build (TODO(integration): clap_complete)",
    ]);
    Ok(1)
}
