//! Auto-update / version upgrade helpers.
//! From reference/packages/opencode/src/cli/upgrade.ts and
//! reference/packages/opencode/src/installation/index.ts.

use crate::version::INSTALLATION_VERSION;

/// From reference/packages/opencode/src/installation/index.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseType {
    Major,
    Minor,
    Patch,
}

fn release_part(version: &str, index: usize) -> u64 {
    version
        .split('.')
        .nth(index)
        .and_then(|part| part.parse().ok())
        .unwrap_or(0)
}

/// Mirrors `Installation.getReleaseType(current, latest)`.
pub fn get_release_type(current: &str, latest: &str) -> ReleaseType {
    let curr_major = release_part(current, 0);
    let curr_minor = release_part(current, 1);
    let new_major = release_part(latest, 0);
    let new_minor = release_part(latest, 1);
    if new_major > curr_major {
        ReleaseType::Major
    } else if new_minor > curr_minor {
        ReleaseType::Minor
    } else {
        ReleaseType::Patch
    }
}

/// Fetch the latest published opencode version from the GitHub releases API.
/// Mirrors `Installation.latest(method)` minus the install-method logic.
pub async fn fetch_latest() -> Option<String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/sst/opencode/releases/latest")
        .header("User-Agent", format!("opencode/{INSTALLATION_VERSION}"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    let tag = body.get("tag_name")?.as_str()?;
    Some(tag.strip_prefix('v').unwrap_or(tag).to_string())
}

/// Whether autoupdate is disabled for this invocation.
/// From reference/packages/core/src/flag/flag.ts (`OPENCODE_DISABLE_AUTOUPDATE`).
pub fn autoupdate_disabled() -> bool {
    matches!(
        std::env::var("OPENCODE_DISABLE_AUTOUPDATE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "true" | "1"
    )
}

/// Whether updates should always be surfaced.
/// From reference flag `OPENCODE_ALWAYS_NOTIFY_UPDATE`.
pub fn always_notify_update() -> bool {
    matches!(
        std::env::var("OPENCODE_ALWAYS_NOTIFY_UPDATE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "true" | "1"
    )
}
