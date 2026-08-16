//! `opencode completion`
//! Mirrors the yargs `.completion("completion", "generate shell completion script")`.

use std::io::Write;

use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::cli::args::Cli;

/// Generate a completion script from the canonical clap command tree.
///
/// Keeping this separate from [`run`] makes the output deterministic and lets
/// tests exercise every shell without mutating the process `SHELL` variable.
pub fn generate_script(shell: Shell) -> String {
    let mut command = Cli::command();
    let mut output = Vec::new();
    generate(shell, &mut command, "opencode", &mut output);
    String::from_utf8(output).expect("clap_complete output is UTF-8")
}

fn shell_from_env_value(value: Option<std::ffi::OsString>) -> Shell {
    value
        .as_ref()
        .and_then(|value| Shell::from_shell_path(value))
        .unwrap_or(Shell::Bash)
}

fn shell_from_environment() -> Shell {
    shell_from_env_value(std::env::var_os("SHELL"))
}

pub fn run() -> anyhow::Result<i32> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(generate_script(shell_from_environment()).as_bytes())?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap::{Command, CommandFactory};

    use crate::cli::args::Cli;

    use super::{generate_script, shell_from_env_value};
    use clap_complete::Shell;

    fn collect_visible_command_tokens(command: &Command, tokens: &mut Vec<String>) {
        for subcommand in command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set())
        {
            tokens.push(subcommand.get_name().to_owned());
            tokens.extend(subcommand.get_visible_aliases().map(str::to_owned));
            collect_visible_command_tokens(subcommand, tokens);
        }
    }

    #[test]
    fn bash_completion_contains_commands_and_options() {
        let script = generate_script(Shell::Bash);

        assert!(script.contains("opencode"));
        assert!(script.contains("completion"));
        assert!(script.contains("run"));
        assert!(script.contains("--model"));
        assert!(script.contains("--continue"));
    }

    #[test]
    fn zsh_completion_contains_nested_command_options() {
        let script = generate_script(Shell::Zsh);

        assert!(script.contains("_opencode"));
        assert!(script.contains("session"));
        assert!(script.contains("--format"));
        assert!(script.contains("--max-count"));
    }

    #[test]
    fn fish_completion_contains_arguments_and_aliases() {
        let script = generate_script(Shell::Fish);

        assert!(script.contains("complete -c opencode"));
        assert!(script.contains("project"));
        assert!(script.contains("-l model"));
        assert!(script.contains("providers"));
        assert!(script.contains("auth"));
    }

    #[test]
    fn shell_detection_supports_all_clap_complete_shells() {
        let cases = [
            ("/bin/bash", Shell::Bash),
            ("/usr/bin/elvish", Shell::Elvish),
            ("/usr/bin/fish", Shell::Fish),
            ("/usr/bin/pwsh", Shell::PowerShell),
            ("/usr/bin/zsh", Shell::Zsh),
        ];

        for (path, expected) in cases {
            assert_eq!(
                shell_from_env_value(Some(OsString::from(path))),
                expected,
                "shell path {path}"
            );
        }

        assert_eq!(shell_from_env_value(None), Shell::Bash);
        assert_eq!(
            shell_from_env_value(Some(OsString::from("/usr/bin/nu"))),
            Shell::Bash
        );
    }

    #[test]
    fn every_shell_completion_emits_the_visible_command_tree() {
        let command = Cli::command();
        let visible_top_level: Vec<_> = command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set())
            .map(|subcommand| subcommand.get_name())
            .collect();

        assert_eq!(
            visible_top_level,
            vec![
                "completion",
                "acp",
                "mcp",
                "attach",
                "run",
                "debug",
                "providers",
                "agent",
                "upgrade",
                "uninstall",
                "serve",
                "web",
                "models",
                "stats",
                "export",
                "import",
                "github",
                "pr",
                "session",
                "plugin",
                "db",
            ]
        );

        let mut tokens = Vec::new();
        collect_visible_command_tokens(&command, &mut tokens);

        for shell in [
            Shell::Bash,
            Shell::Elvish,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Zsh,
        ] {
            let script = generate_script(shell);

            for token in &tokens {
                assert!(
                    script.contains(token),
                    "{shell:?} completion is missing visible command or alias {token}"
                );
            }
        }
    }
}
