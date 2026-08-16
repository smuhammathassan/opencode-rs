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
}

pub async fn run(_cli: &Cli, args: &UninstallArgs) -> anyhow::Result<i32> {
    ui::empty();
    ui::println(&[&ui::logo(Some("  "))]);
    ui::empty();
    ui::println(&["◇  Uninstall OpenCode"]);
    let paths = GlobalPaths::load();
    let home = paths.home();
    let targets = collect_removal_targets(args, &paths);

    ui::println(&["│  The following will be removed:"]);
    for target in &targets {
        if !target.path.exists() {
            continue;
        }
        let size = format_size(directory_size(&target.path));
        let status = if target.keep {
            format!("{} (keeping)", Style::TEXT_DIM)
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
    ui::println(&["└  Done"]);
    Ok(if failed { 1 } else { 0 })
}
