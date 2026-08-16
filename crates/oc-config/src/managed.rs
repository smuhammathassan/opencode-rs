// From reference/packages/opencode/src/config/managed.ts.

use std::path::{Path, PathBuf};

/// Explicit managed-config directory override.
///
/// This is useful for enterprise deployments that mount policy elsewhere and
/// makes the discovery path deterministic in tests. An empty value disables
/// managed-config discovery.
pub const MANAGED_CONFIG_DIR_ENV: &str = "OPENCODE_MANAGED_CONFIG_DIR";

/// Test-only compatibility override retained for the crate's integration
/// fixtures. `OPENCODE_MANAGED_CONFIG_DIR` takes precedence when both exist.
pub const TEST_MANAGED_CONFIG_DIR_ENV: &str = "OPENCODE_TEST_MANAGED_CONFIG_DIR";

/// Keys injected by macOS/MDM into the managed plist that are not OpenCode config.
const PLIST_META: [&str; 6] = [
    "PayloadDisplayName",
    "PayloadIdentifier",
    "PayloadType",
    "PayloadUUID",
    "PayloadVersion",
    "_manualProfile",
];

/// `ConfigManaged.parseManagedPlist` — strips MDM metadata keys from the JSON
/// serialization of a managed preferences plist.
pub fn parse_managed_plist(json: &str) -> Result<String, serde_json::Error> {
    let mut value = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json)?;
    for key in PLIST_META {
        value.shift_remove(key);
    }
    serde_json::to_string(&value)
}

/// Returns the system-managed configuration directory for the current OS.
///
/// The environment overrides are checked before platform defaults so callers
/// can exercise every platform's loading behavior without changing the host
/// operating system.
pub fn managed_config_dir() -> Option<PathBuf> {
    if let Some(value) = env_path(MANAGED_CONFIG_DIR_ENV) {
        return value;
    }
    if let Some(value) = env_path(TEST_MANAGED_CONFIG_DIR_ENV) {
        return value;
    }

    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from("/Library/Managed Preferences"))
    }
    #[cfg(target_os = "windows")]
    {
        Some(
            PathBuf::from(
                std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()),
            )
            .join("opencode"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some(PathBuf::from("/etc/opencode"))
    }
}

fn env_path(name: &str) -> Option<Option<PathBuf>> {
    match std::env::var_os(name) {
        None => None,
        Some(value) if value.is_empty() => Some(None),
        Some(value) => Some(Some(PathBuf::from(value))),
    }
}

/// Candidate files are ordered from lowest to highest precedence.
///
/// JSON files are supported on all platforms. The plist names cover the
/// usual macOS MDM payload names and are also accepted under an override on
/// other platforms when their contents are JSON, which keeps tests portable.
pub fn config_files(directory: &Path) -> Vec<PathBuf> {
    [
        "config.json",
        "opencode.json",
        "opencode.jsonc",
        "opencode.plist",
        "ai.opencode.plist",
        "com.opencode.plist",
    ]
    .into_iter()
    .map(|name| directory.join(name))
    .collect()
}

/// Reads a managed config file and normalizes plist metadata away.
///
/// macOS uses `plutil` because it is the system's canonical plist parser. On
/// other platforms a plist candidate may still contain JSON (handy for
/// portable deployment tests), while XML plist input returns a clear error.
pub fn read_config(path: &Path) -> std::io::Result<String> {
    let is_plist = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("plist"));
    if !is_plist {
        return std::fs::read_to_string(path);
    }

    let raw = std::fs::read_to_string(path)?;

    #[cfg(target_os = "macos")]
    let json = {
        let output = std::process::Command::new("plutil")
            .args(["-convert", "json", "-o", "-", "--"])
            .arg(path)
            .output()?;
        if output.status.success() {
            String::from_utf8(output.stdout).map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?
        } else {
            // Keep JSON-encoded fixtures and enterprise export files usable
            // on macOS even when they are given a `.plist` suffix.
            raw.clone()
        }
    };

    #[cfg(not(target_os = "macos"))]
    let json = raw;

    parse_managed_plist(&json).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("managed plist is not valid JSON: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{config_files, parse_managed_plist, read_config};
    use std::path::Path;

    #[test]
    fn strips_mdm_metadata_keys() {
        let out = parse_managed_plist(
            r#"{"PayloadDisplayName":"x","PayloadUUID":"u","_manualProfile":true,"share":"disabled","model":"mdm/model"}"#,
        )
        .expect("parse");
        let value: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(value["share"], "disabled");
        assert_eq!(value["model"], "mdm/model");
        assert!(value.get("PayloadUUID").is_none());
        assert!(value.get("_manualProfile").is_none());
    }

    #[test]
    fn discovers_cross_platform_candidate_names_in_precedence_order() {
        let files = config_files(Path::new("/managed"));
        let names: Vec<_> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "config.json",
                "opencode.json",
                "opencode.jsonc",
                "opencode.plist",
                "ai.opencode.plist",
                "com.opencode.plist"
            ]
        );
    }

    #[test]
    fn reads_json_encoded_plist_on_every_platform() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("ai.opencode.plist");
        std::fs::write(
            &path,
            r#"{"PayloadType":"Configuration","PayloadUUID":"uuid","model":"mdm/model"}"#,
        )
        .expect("write plist fixture");
        let value: serde_json::Value =
            serde_json::from_str(&read_config(&path).expect("read plist")).expect("json");
        assert_eq!(value["model"], "mdm/model");
        assert!(value.get("PayloadUUID").is_none());
    }
}
