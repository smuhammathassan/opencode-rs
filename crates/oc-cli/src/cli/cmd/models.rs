//! `opencode models [provider]`
//! From reference/packages/opencode/src/cli/cmd/models.ts.
//!
//! Lists the connected provider catalog: the embedded models.dev snapshot plus
//! providers/models declared in `opencode.json`, mirroring `Provider.list()`.

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

    let providers = build_provider_catalog(&ctx)?;
    if providers.is_empty() {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "!  ",
            Style::TEXT_NORMAL,
            "models database is empty; run `opencode models --refresh` to fetch it",
        ]);
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let print_provider =
        |out: &mut dyn Write, provider_id: &str, verbose: bool| -> anyhow::Result<()> {
            let provider = providers
                .get(provider_id)
                .ok_or_else(|| CliError::new(format!("Provider not found: {provider_id}")))?;
            let mut model_ids: Vec<&String> = provider.models.keys().collect();
            model_ids.sort();
            for model_id in model_ids {
                writeln!(out, "{provider_id}/{model_id}")?;
                if verbose {
                    let model = provider.models.get(model_id).expect("key present");
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

/// Build the merged provider catalog: embedded models.dev snapshot + the
/// `opencode.json` provider section and allowlists, mirroring
/// `Provider.list()` from `reference/packages/opencode/src/provider/provider.ts`.
fn build_provider_catalog(
    ctx: &Context,
) -> anyhow::Result<indexmap::IndexMap<String, oc_provider::provider::Info>> {
    let state = oc_config::load::load_instance_state(&oc_config::load::LoadOptions {
        directory: ctx.directory.to_string_lossy().into_owned(),
        worktree: Some(ctx.worktree.to_string_lossy().into_owned()),
        ..Default::default()
    })?;
    let config = serde_json::to_value(&state.config)?;
    let provider_values = config
        .get("provider")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let providers = provider_values
        .iter()
        .filter_map(|(id, value)| {
            serde_json::from_value::<oc_provider::provider::registry::ConfigProvider>(value.clone())
                .ok()
                .map(|mut provider| {
                    if provider.id.is_none() {
                        provider.id = Some(id.clone());
                    }
                    (id.clone(), provider)
                })
        })
        .collect::<indexmap::IndexMap<_, _>>();
    let disabled = string_list(config.get("disabled_providers"));
    let enabled = string_list(config.get("enabled_providers"));

    let catalog = oc_provider::models_dev::snapshot()?;
    let envs = std::env::vars()
        .map(|(key, value)| (key, Some(value)))
        .collect::<std::collections::BTreeMap<_, _>>();
    use oc_provider::auth::AuthStore;
    let auths = oc_provider::auth::FileAuthStore::new(&ctx.paths.data).all()?;
    let input = oc_provider::provider::registry::RegistryInput {
        catalog: &catalog,
        config: oc_provider::provider::registry::ConfigInput {
            provider: &providers,
            disabled_providers: disabled.as_deref(),
            enabled_providers: enabled.as_deref(),
        },
        envs: &envs,
        auths: &auths,
        enable_experimental_models: false,
    };
    oc_provider::provider::registry::build_registry(&input)
        .map_err(|error| anyhow::anyhow!("failed to build provider catalog: {error}"))
}

fn string_list(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    value.and_then(serde_json::Value::as_array).map(|values| {
        values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect()
    })
}
