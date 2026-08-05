//! The `opencode` binary entrypoint.
//! Mirrors `reference/packages/opencode/src/index.ts`.

use clap::Parser;

use oc_cli::cli::args::Cli;
use oc_cli::cli::cmd;
use oc_cli::cli::ui;

const SUBCOMMANDS: &[&str] = &[
    "completion",
    "acp",
    "mcp",
    "attach",
    "run",
    "debug",
    "providers",
    "auth",
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
    "plug",
    "db",
    "generate",
    "console",
];

/// Mirrors the `show()` helper in index.ts: write to stderr, prepending the
/// logo unless the output already starts with `opencode `.
fn show(out: &str) {
    use std::io::Write;
    let text = out.trim_start();
    if !text.starts_with("opencode ") {
        let logo = ui::logo(None);
        let mut stderr = std::io::stderr();
        let _ = writeln!(stderr, "{logo}\n");
        let _ = stderr.write_all(text.as_bytes());
        let _ = stderr.write_all(b"\n");
        return;
    }
    let _ = std::io::stderr().write_all(out.as_bytes());
}

fn is_subcommand_help() -> bool {
    let first = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));
    match first.as_deref() {
        Some(name) => SUBCOMMANDS.contains(&name),
        None => false,
    }
}

fn render_help(err: &clap::Error) {
    let text = err.to_string();
    if is_subcommand_help() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), text.as_bytes());
    } else {
        show(&text);
    }
}

fn main() {
    std::env::set_var("AGENT", "1");
    std::env::set_var("OPENCODE", "1");
    std::env::set_var("OPENCODE_PID", std::process::id().to_string());

    oc_cli::cli::heap::start();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            use clap::error::ErrorKind;
            match err.kind() {
                ErrorKind::DisplayVersion => {
                    println!("{}", oc_cli::VERSION);
                    std::process::exit(0);
                }
                ErrorKind::DisplayHelp => {
                    render_help(&err);
                    std::process::exit(0);
                }
                _ => {
                    // Unknown arguments and similar parse failures exit 1 and
                    // (for unknown arguments) show help, mirroring the
                    // reference `.fail()` handler.
                    let _ = err.print();
                    std::process::exit(1);
                }
            }
        }
    };

    if cli.global.version {
        println!("{}", oc_cli::VERSION);
        std::process::exit(0);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            ui::error(&format!("failed to start runtime: {err}"));
            std::process::exit(1);
        }
    };
    let code = runtime.block_on(cmd::dispatch(&cli));
    std::process::exit(code);
}
