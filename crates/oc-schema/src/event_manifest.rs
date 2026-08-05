//! From reference/packages/schema/src/event-manifest.ts

use crate::event::{latest as event_latest, Definition};

fn v1_durable_definitions() -> Vec<Definition> {
    crate::session_v1::Definitions
        .iter()
        .filter(|d| d.durable.is_some())
        .cloned()
        .collect()
}

fn v1_live_definitions() -> Vec<Definition> {
    crate::session_v1::Definitions
        .iter()
        .filter(|d| d.durable.is_none())
        .cloned()
        .collect()
}

fn core_definitions() -> Vec<Definition> {
    let mut out = Vec::new();
    out.extend(v1_durable_definitions());
    out.extend(crate::session_event::DEFINITIONS.to_vec());
    out
}

fn foundation_definitions() -> Vec<Definition> {
    let mut out = Vec::new();
    out.extend(crate::models_dev::Definitions.to_vec());
    out.extend(crate::integration::Definitions.to_vec());
    out.extend(crate::catalog::Definitions.to_vec());
    out.extend(core_definitions());
    out
}

fn feature_definitions() -> Vec<Definition> {
    let mut out = Vec::new();
    out.extend(crate::filesystem::Definitions.to_vec());
    out.extend(crate::reference::Definitions.to_vec());
    out.extend(crate::permission::Definitions.to_vec());
    out.extend(crate::plugin::Definitions.to_vec());
    out.extend(crate::project_directories::Definitions.to_vec());
    out.extend(crate::filesystem_watcher::Definitions.to_vec());
    out.extend(crate::pty::Definitions.to_vec());
    out.extend(crate::question::Definitions.to_vec());
    out
}

/// `EventManifest.ServerDefinitions`.
pub fn server_definitions() -> Vec<Definition> {
    let mut out = Vec::new();
    out.extend(foundation_definitions());
    out.extend(feature_definitions());
    out.extend(crate::session_todo::Definitions.to_vec());
    out
}

/// `EventManifest.Definitions` — the full ordered event inventory.
pub fn definitions() -> Vec<Definition> {
    let mut out = Vec::new();
    out.extend(foundation_definitions());
    out.extend(v1_live_definitions());
    out.extend(crate::installation_event::DEFINITIONS.to_vec());
    out.extend(feature_definitions());
    out.extend(crate::session_todo::Definitions.to_vec());
    out.extend(crate::lsp_event::DEFINITIONS.to_vec());
    out.extend(crate::permission_v1::Definitions.to_vec());
    out.extend(crate::tui_event::DEFINITIONS.to_vec());
    out.extend(crate::mcp_event::DEFINITIONS.to_vec());
    out.extend(crate::legacy_event::DEFINITIONS.to_vec());
    out.extend(crate::project::Definitions.to_vec());
    out.extend(crate::session_status_event::DEFINITIONS.to_vec());
    out.extend(crate::question_v1::Definitions.to_vec());
    out.extend(crate::session_compaction_event::DEFINITIONS.to_vec());
    out.extend(crate::vcs_event::DEFINITIONS.to_vec());
    out.extend(crate::workspace_event::DEFINITIONS.to_vec());
    out.extend(crate::worktree_event::DEFINITIONS.to_vec());
    out.extend(crate::server_event::DEFINITIONS.to_vec());
    out
}

/// `EventManifest.Latest` — one definition per type, preferring the newest durable version.
pub fn latest() -> Vec<Definition> {
    event_latest(&definitions())
}
