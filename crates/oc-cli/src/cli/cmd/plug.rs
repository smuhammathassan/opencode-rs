//! `opencode plugin <module>` (alias `plug`)
//! From reference/packages/opencode/src/cli/cmd/plug.ts.

use crate::cli::args::{Cli, PluginArgs};
use crate::cli::context::{Context, Vcs};
use crate::cli::ui;

pub async fn run(_cli: &Cli, args: &PluginArgs) -> anyhow::Result<i32> {
    let module = args.module.trim();
    if module.is_empty() {
        ui::error("module is required");
        return Ok(1);
    }

    ui::println(&[&format!("◇  Install plugin {module}")]);
    let ctx = Context::load(std::env::current_dir()?)?;
    let spec = module.to_string();
    let installed = tokio::task::spawn_blocking({
        let spec = spec.clone();
        move || oc_plugin::install::install_plugin(&spec)
    })
    .await?;
    let target = match installed {
        oc_plugin::install::InstallResult::Ok { target } => target,
        oc_plugin::install::InstallResult::Failed { error } => {
            return Err(anyhow::anyhow!(error));
        }
    };

    let targets = tokio::task::spawn_blocking({
        let target = target.clone();
        move || oc_plugin::install::read_plugin_manifest(&target)
    })
    .await?;
    let targets = match targets {
        oc_plugin::install::ManifestResult::Ok { targets } => targets,
        oc_plugin::install::ManifestResult::ReadFailed { file, error } => {
            return Err(anyhow::anyhow!(
                "failed to read plugin manifest {file}: {error}"
            ));
        }
        oc_plugin::install::ManifestResult::NoTargets { file } => {
            return Err(anyhow::anyhow!(
                "plugin manifest {file} does not expose a server or TUI target"
            ));
        }
    };
    if targets.is_empty() {
        return Err(anyhow::anyhow!(
            "plugin {module} exposes no installable targets"
        ));
    }

    let input = oc_plugin::install::PatchInput {
        spec,
        targets,
        force: args.force,
        global: args.global,
        vcs: matches!(ctx.project.vcs, Vcs::Git).then_some("git".to_string()),
        worktree: ctx.worktree.to_string_lossy().into_owned(),
        directory: ctx.directory.to_string_lossy().into_owned(),
        config: None,
    };
    let result =
        tokio::task::spawn_blocking(move || oc_plugin::install::patch_plugin_config(&input))
            .await?;
    match result {
        oc_plugin::install::PatchResult::Ok { dir, items } => {
            for item in items {
                let action = match item.mode {
                    oc_plugin::install::PatchMode::Noop => "already configured",
                    oc_plugin::install::PatchMode::Add => "added",
                    oc_plugin::install::PatchMode::Replace => "updated",
                };
                ui::println(&[&format!("│  {action} {} in {}", item.kind, item.file)]);
            }
            ui::println(&[&format!("└  Plugin installed in {dir}")]);
            Ok(0)
        }
        oc_plugin::install::PatchResult::Failed { dir, kind, message } => Err(anyhow::anyhow!(
            "failed to patch {kind} plugin config in {dir}: {message}"
        )),
    }
}
