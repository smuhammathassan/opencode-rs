//! `opencode uninstall`
//! From reference/packages/opencode/src/cli/cmd/uninstall.ts.

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use crate::cli::args::{Cli, UninstallArgs};
use crate::cli::paths::GlobalPaths;
use crate::cli::ui::{self, Style};

struct RemovalTarget {
    path: PathBuf,
    label: &'static str,
    keep: bool,
}

/// How the running binary was installed, mirroring
/// `Installation.method()` in the reference. The uninstall surface only needs
/// the curl/unknown split plus package-manager hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallationMethod {
    Curl,
    Npm,
    Brew,
    Choco,
    Scoop,
    Unknown,
}

fn installation_method() -> InstallationMethod {
    std::env::current_exe()
        .map(|exe| installation_method_from_path(&exe.to_string_lossy()))
        .unwrap_or(InstallationMethod::Unknown)
}

fn installation_method_from_path(text: &str) -> InstallationMethod {
    if text.contains(".opencode/bin") {
        InstallationMethod::Curl
    } else if text.contains("node_modules") {
        InstallationMethod::Npm
    } else if text.contains("Cellar") || text.contains("homebrew") {
        InstallationMethod::Brew
    } else if text.contains("chocolatey") {
        InstallationMethod::Choco
    } else if text.contains("scoop") {
        InstallationMethod::Scoop
    } else {
        InstallationMethod::Unknown
    }
}

/// Shell config candidates for the current `$SHELL`, mirroring
/// `getShellConfigFile()` in the reference uninstall command.
fn shell_config_candidates() -> Vec<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let base = Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "bash".to_string());
    let home = GlobalPaths::load().home();
    let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    match base.as_str() {
        "fish" => vec![xdg_config.join("fish/config.fish")],
        "zsh" => vec![
            home.join(".zshrc"),
            home.join(".zshenv"),
            xdg_config.join("zsh/.zshrc"),
            xdg_config.join("zsh/.zshenv"),
        ],
        _ => vec![
            home.join(".bashrc"),
            home.join(".bash_profile"),
            home.join(".profile"),
        ],
    }
}

/// Find the first existing shell config that contains opencode PATH lines,
/// mirroring the reference `# opencode` / `.opencode/bin` marker search.
fn find_shell_config_with_opencode() -> Option<PathBuf> {
    shell_config_candidates().into_iter().find(|file| {
        std::fs::read_to_string(file)
            .map(|content| content.contains("# opencode") || content.contains(".opencode/bin"))
            .unwrap_or(false)
    })
}

/// Remove opencode PATH lines from a shell config, mirroring
/// `cleanShellConfig()` in the reference: drop the `# opencode` marker plus
/// the line that follows it when it is the PATH export, and any standalone
/// PATH/fish_add_path lines referencing `.opencode`.
fn clean_shell_config(path: &Path) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut filtered: Vec<&str> = Vec::new();
    let mut skip = false;
    for line in content.split('\n') {
        let trimmed = line.trim();
        if trimmed == "# opencode" {
            skip = true;
            continue;
        }
        if skip {
            skip = false;
            if trimmed.contains(".opencode/bin") || trimmed.contains("fish_add_path") {
                continue;
            }
        }
        if (trimmed.starts_with("export PATH=") && trimmed.contains(".opencode/bin"))
            || (trimmed.starts_with("fish_add_path") && trimmed.contains(".opencode"))
        {
            continue;
        }
        filtered.push(line);
    }
    while filtered.last().is_some_and(|l| l.trim().is_empty()) {
        filtered.pop();
    }
    let mut output = filtered.join("\n");
    output.push('\n');
    std::fs::write(path, output)
}

fn package_manager_hint(method: InstallationMethod) -> Option<&'static str> {
    match method {
        InstallationMethod::Npm => Some("npm uninstall -g opencode-ai"),
        InstallationMethod::Brew => Some("brew uninstall opencode"),
        InstallationMethod::Choco => Some("choco uninstall opencode"),
        InstallationMethod::Scoop => Some("scoop uninstall opencode"),
        InstallationMethod::Curl | InstallationMethod::Unknown => None,
    }
}

fn collect_removal_targets(args: &UninstallArgs, paths: &GlobalPaths) -> Vec<RemovalTarget> {
    vec![
        RemovalTarget {
            path: paths.data.clone(),
            label: "Data",
            keep: args.keep_data,
        },
        RemovalTarget {
            path: paths.cache.clone(),
            label: "Cache",
            keep: false,
        },
        RemovalTarget {
            path: paths.config.clone(),
            label: "Config",
            keep: args.keep_config,
        },
        RemovalTarget {
            path: paths.state.clone(),
            label: "State",
            keep: false,
        },
    ]
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn shorten_path(path: &std::path::Path, home: &std::path::Path) -> String {
    path.strip_prefix(home)
        .map(|rest| {
            if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!(
                    "~{}",
                    std::path::MAIN_SEPARATOR.to_string() + &rest.to_string_lossy()
                )
            }
        })
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

fn directory_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                total += directory_size(&path);
            } else if let Ok(meta) = std::fs::metadata(&path) {
                total += meta.len();
            }
        }
    }
    total
}

fn confirm_from_stdin() -> io::Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_formatting_is_bounded_and_human_readable() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
    }

    #[cfg(unix)]
    #[test]
    fn paths_are_shortened_only_under_home() {
        let home = Path::new("/tmp/home");
        assert_eq!(
            shorten_path(Path::new("/tmp/home/.cache"), home),
            "~/.cache"
        );
        assert_eq!(
            shorten_path(Path::new("/tmp/home-other"), home),
            "/tmp/home-other"
        );
    }

    fn temp_config(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "opencode-uninstall-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clean_shell_config_removes_marker_and_path_lines() {
        let dir = temp_config("clean");
        let file = dir.join("shellrc");
        std::fs::write(
            &file,
            "export PATH=/usr/bin\n# opencode\nexport PATH=\"$HOME/.opencode/bin:$PATH\"\nalias ll='ls -l'\n",
        )
        .unwrap();
        clean_shell_config(&file).unwrap();
        let cleaned = std::fs::read_to_string(&file).unwrap();
        assert!(!cleaned.contains("# opencode"));
        assert!(!cleaned.contains(".opencode/bin"));
        assert!(cleaned.contains("export PATH=/usr/bin"));
        assert!(cleaned.contains("alias ll"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_shell_config_keeps_unrelated_lines_intact() {
        let dir = temp_config("keep");
        let file = dir.join("shellrc");
        std::fs::write(
            &file,
            "export PATH=/usr/local/bin\n# some other tool\nexport FOO=1\n",
        )
        .unwrap();
        clean_shell_config(&file).unwrap();
        let cleaned = std::fs::read_to_string(&file).unwrap();
        assert!(cleaned.contains("# some other tool"));
        assert!(cleaned.contains("export FOO=1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installation_method_classifies_common_locations() {
        // cargo target builds are unknown, matching the upgrade gate.
        assert_eq!(
            installation_method_from_path("/repo/target/debug/opencode"),
            InstallationMethod::Unknown
        );
        assert_eq!(
            installation_method_from_path("/home/u/.opencode/bin/opencode"),
            InstallationMethod::Curl
        );
        assert_eq!(
            installation_method_from_path(
                "/nvm/v22/lib/node_modules/@opencode-ai/cli/bin/opencode"
            ),
            InstallationMethod::Npm
        );
        assert_eq!(
            installation_method_from_path("/opt/homebrew/Cellar/opencode/1.0/bin/opencode"),
            InstallationMethod::Brew
        );
    }
}

pub async fn run(_cli: &Cli, args: &UninstallArgs) -> anyhow::Result<i32> {
    ui::empty();
    ui::println(&[&ui::logo(Some("  "))]);
    ui::empty();
    ui::println(&["◇  Uninstall OpenCode"]);
    let paths = GlobalPaths::load();
    let home = paths.home();
    let method = installation_method();
    let method_label = match method {
        InstallationMethod::Curl => "curl",
        InstallationMethod::Npm => "npm",
        InstallationMethod::Brew => "brew",
        InstallationMethod::Choco => "choco",
        InstallationMethod::Scoop => "scoop",
        InstallationMethod::Unknown => "unknown",
    };
    ui::println(&["│", &format!("  Installation method: {method_label}")]);
    let targets = collect_removal_targets(args, &paths);
    let binary = (method == InstallationMethod::Curl)
        .then(std::env::current_exe)
        .and_then(Result::ok);
    let shell_config = (method == InstallationMethod::Curl)
        .then(find_shell_config_with_opencode)
        .flatten();

    ui::println(&["│  The following will be removed:"]);
    for target in &targets {
        if !target.path.exists() {
            continue;
        }
        let size = format_size(directory_size(&target.path));
        let status = if target.keep {
            format!("{} (keeping){}", Style::TEXT_DIM, Style::TEXT_NORMAL)
        } else {
            String::new()
        };
        let prefix = if target.keep { "○" } else { "✓" };
        ui::println(&[
            "│  ",
            &format!(
                "{prefix} {}: {} {}({size}){}{}",
                target.label,
                shorten_path(&target.path, &home),
                Style::TEXT_DIM,
                status,
                Style::TEXT_NORMAL
            ),
        ]);
    }
    if let Some(binary) = &binary {
        ui::println(&["│  ", &format!("✓ Binary: {}", shorten_path(binary, &home))]);
    }
    if let Some(config) = shell_config.as_ref() {
        ui::println(&[
            "│  ",
            &format!("✓ Shell PATH in {}", shorten_path(config, &home)),
        ]);
    }
    if let Some(hint) = package_manager_hint(method) {
        ui::println(&["│  ", &format!("✓ Package: {hint}")]);
    }

    if args.dry_run {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "▲  ",
            Style::TEXT_NORMAL,
            "Dry run - no changes made",
        ]);
        ui::println(&["└  Done"]);
        return Ok(0);
    }

    if !args.force {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "?  ",
            Style::TEXT_NORMAL,
            "Are you sure you want to uninstall? [y/N]",
        ]);
    }
    if !args.force && !confirm_from_stdin()? {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "?  ",
            Style::TEXT_NORMAL,
            "confirmation required; no changes made",
        ]);
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "!  ",
            Style::TEXT_NORMAL,
            "pass --force to proceed without confirmation (or --dry-run to preview)",
        ]);
        return Ok(2);
    }

    let mut failed = false;
    for target in &targets {
        if target.keep {
            continue;
        }
        if !target.path.exists() {
            continue;
        }
        match std::fs::remove_dir_all(&target.path) {
            Ok(()) => ui::println(&["│  ", &format!("✓ Removed {}", target.label)]),
            Err(err) => {
                failed = true;
                ui::println(&[
                    Style::TEXT_DANGER_BOLD,
                    "✖  ",
                    Style::TEXT_NORMAL,
                    &format!("Failed to remove {}: {}", target.label, err),
                ])
            }
        }
    }
    if let Some(config) = shell_config.as_ref() {
        match clean_shell_config(config) {
            Ok(()) => ui::println(&["│  ", "✓ Cleaned shell PATH entries"]),
            Err(err) => {
                failed = true;
                ui::println(&[
                    Style::TEXT_DANGER_BOLD,
                    "✖  ",
                    Style::TEXT_NORMAL,
                    &format!("Failed to clean shell config: {err}"),
                ])
            }
        }
    }
    if method == InstallationMethod::Curl {
        if let Some(binary) = &binary {
            ui::empty();
            ui::println(&["│  To finish removing the binary, run:"]);
            ui::println(&["│", &format!("  rm \"{}\"", binary.display())]);
            let bin_dir = binary.parent();
            if bin_dir.is_some_and(|dir| dir.to_string_lossy().contains(".opencode")) {
                ui::println(&[
                    "│",
                    &format!("  rmdir \"{}\" 2>/dev/null", bin_dir.unwrap().display()),
                ]);
            }
        }
    }
    if let Some(hint) = package_manager_hint(method) {
        ui::println(&["│  ", &format!("You may need to run: {hint}")]);
    }
    ui::println(&["└  Done"]);
    ui::empty();
    ui::println(&["└  Thank you for using OpenCode!"]);
    Ok(if failed { 1 } else { 0 })
}
