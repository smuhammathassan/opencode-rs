//! `opencode plugin <module>` (alias `plug`)
//! From reference/packages/opencode/src/cli/cmd/plug.ts.

use crate::cli::args::{Cli, PluginArgs};
use crate::cli::effect_cmd::not_wired;
use crate::cli::ui;

pub async fn run(_cli: &Cli, args: &PluginArgs) -> anyhow::Result<i32> {
    let module = args.module.trim();
    if module.is_empty() {
        ui::error("module is required");
        return Ok(1);
    }

    ui::println(&[&format!("◇  Install plugin {module}")]);
    let _ = (args.global, args.force);
    // TODO(integration): install the plugin package and patch the config via
    // `oc_plugin` (installPlugin + patchPluginConfig in plug.ts).
    Err(not_wired("plugin installation is not yet wired in this build (TODO(integration): oc-plugin/oc-config)"))
}
