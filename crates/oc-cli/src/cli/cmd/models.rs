//! `opencode models [provider]`
//! From reference/packages/opencode/src/cli/cmd/models.ts.

use std::io::Write;

use crate::cli::args::{Cli, ModelsArgs};
use crate::cli::context::Context;
use crate::cli::effect_cmd::CliError;
use crate::cli::models_dev::ModelsDev;
use crate::cli::ui::{self, Style};

pub async fn run(_cli: &Cli, args: &ModelsArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;

    if args.refresh {
        match ModelsDev::refresh(&ctx.paths, true).await {
            Ok(()) => ui::println(&[
                Style::TEXT_SUCCESS_BOLD,
                "Models cache refreshed",
                Style::TEXT_NORMAL,
            ]),
            Err(err) => {
                ui::println(&[
                    Style::TEXT_WARNING_BOLD,
                    "!  ",
                    Style::TEXT_NORMAL,
                    &format!("failed to refresh models cache: {err}"),
                ]);
            }
        }
    }

    let db = ModelsDev::load(&ctx.paths).unwrap_or_default();
    if db.providers.is_empty() {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "!  ",
            Style::TEXT_NORMAL,
            "models database is empty; run `opencode models --refresh` to fetch it",
        ]);
    }

    let providers = db.providers;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let print_provider =
        |out: &mut dyn Write, provider_id: &str, verbose: bool| -> anyhow::Result<()> {
            let provider = providers
                .get(provider_id)
                .ok_or_else(|| CliError::new(format!("Provider not found: {provider_id}")))?;
            for (model_id, model) in &provider.models {
                writeln!(out, "{provider_id}/{model_id}")?;
                if verbose {
                    writeln!(out, "{}", serde_json::to_string_pretty(model)?)?;
                }
            }
            Ok(())
        };

    if let Some(provider) = &args.provider {
        print_provider(&mut out, provider, args.verbose)?;
        return Ok(0);
    }

    let mut ids: Vec<&String> = providers.keys().collect();
    ids.sort_by(|a, b| {
        let a_is_opencode = a.starts_with("opencode");
        let b_is_opencode = b.starts_with("opencode");
        if a_is_opencode && !b_is_opencode {
            return std::cmp::Ordering::Less;
        }
        if !a_is_opencode && b_is_opencode {
            return std::cmp::Ordering::Greater;
        }
        a.cmp(b)
    });

    for id in ids {
        print_provider(&mut out, id, args.verbose)?;
    }
    Ok(0)
}
