//! Auto-update / version upgrade helpers.
//! From reference/packages/opencode/src/cli/upgrade.ts and
//! reference/packages/opencode/src/installation/index.ts.

use oc_config::v1::config::AutoUpdate;
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::version::INSTALLATION_VERSION;

const STARTUP_UPDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const RELEASE_API_BASE: &str = "https://api.github.com/repos/anomalyco/opencode";
const MAX_RELEASE_BYTES: u64 = 512 * 1024 * 1024;

/// A release version accepted by `opencode upgrade`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// From reference/packages/opencode/src/installation/index.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseType {
    Major,
    Minor,
    Patch,
}

/// Normalize a release tag and reject values that are not plain `x.y.z`
/// versions. Upgrade must not pass arbitrary user input to an installer.
pub fn parse_version(input: &str) -> Option<Version> {
    let input = input.trim().strip_prefix('v').unwrap_or(input.trim());
    let core = input.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let version = Version {
        major: parts.next()?.parse().ok()?,
        minor: parts.next()?.parse().ok()?,
        patch: parts.next()?.parse().ok()?,
    };
    parts.next().is_none().then_some(version)
}

pub fn normalize_target(input: &str) -> Option<String> {
    let version = parse_version(input)?;
    Some(format!(
        "{}.{}.{}",
        version.major, version.minor, version.patch
    ))
}

#[derive(Debug, thiserror::Error)]
pub enum UpgradeError {
    #[error("invalid release version: {0}")]
    InvalidVersion(String),
    #[error("release metadata is unsupported: {0}")]
    UnsupportedMetadata(String),
    #[error("the running executable is unavailable: {0}")]
    ExecutableUnavailable(String),
    #[error("downloaded release is too large")]
    ReleaseTooLarge,
    #[error("downloaded release size mismatch: expected {expected} bytes, got {actual}")]
    ReleaseSizeMismatch { expected: u64, actual: u64 },
    #[error("release archive does not contain {0}")]
    BinaryNotFound(String),
    #[error("release download failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("release archive is invalid: {0}")]
    Archive(String),
    #[error("filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("zip archive is invalid: {0}")]
    Zip(#[from] zip::result::ZipError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOs {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseArch {
    X64,
    Arm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleasePlatform {
    pub os: ReleaseOs,
    pub arch: ReleaseArch,
    pub musl: bool,
    pub baseline: bool,
}

impl ReleasePlatform {
    pub fn asset_name(self) -> String {
        let os = match self.os {
            ReleaseOs::Linux => "linux",
            ReleaseOs::Macos => "darwin",
            ReleaseOs::Windows => "windows",
        };
        let arch = match self.arch {
            ReleaseArch::X64 => "x64",
            ReleaseArch::Arm64 => "arm64",
        };
        let mut target = format!("{os}-{arch}");
        if self.baseline {
            target.push_str("-baseline");
        }
        if self.musl {
            target.push_str("-musl");
        }
        let extension = if self.os == ReleaseOs::Linux {
            "tar.gz"
        } else {
            "zip"
        };
        format!("opencode-{target}.{extension}")
    }

    fn binary_name(self) -> &'static str {
        if self.os == ReleaseOs::Windows {
            "opencode.exe"
        } else {
            "opencode"
        }
    }
}

pub fn current_platform() -> Result<ReleasePlatform, UpgradeError> {
    let os = match std::env::consts::OS {
        "linux" => ReleaseOs::Linux,
        "macos" => ReleaseOs::Macos,
        "windows" => ReleaseOs::Windows,
        other => {
            return Err(UpgradeError::UnsupportedMetadata(format!(
                "unsupported operating system {other}"
            )))
        }
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => ReleaseArch::X64,
        "aarch64" => ReleaseArch::Arm64,
        other => {
            return Err(UpgradeError::UnsupportedMetadata(format!(
                "unsupported CPU architecture {other}"
            )))
        }
    };
    if os == ReleaseOs::Windows && arch == ReleaseArch::Arm64 {
        return Err(UpgradeError::UnsupportedMetadata(
            "Windows arm64 release artifacts are not published".into(),
        ));
    }

    let musl = os == ReleaseOs::Linux && linux_uses_musl();
    let baseline = arch == ReleaseArch::X64 && !cpu_supports_avx2();
    Ok(ReleasePlatform {
        os,
        arch,
        musl,
        baseline,
    })
}

fn linux_uses_musl() -> bool {
    if Path::new("/etc/alpine-release").is_file() {
        return true;
    }
    std::process::Command::new("ldd")
        .arg("--version")
        .output()
        .map(|output| {
            let mut text = String::from_utf8_lossy(&output.stdout).to_lowercase();
            text.push_str(&String::from_utf8_lossy(&output.stderr).to_lowercase());
            text.contains("musl")
        })
        .unwrap_or(false)
}

fn cpu_supports_avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        return std::is_x86_feature_detected!("avx2");
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReleaseMetadata {
    #[serde(rename = "tag_name")]
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

pub fn release_api_url(api_base: &str, target: Option<&str>) -> Result<String, UpgradeError> {
    let base = api_base.trim_end_matches('/');
    match target {
        None => Ok(format!("{base}/releases/latest")),
        Some(target) => {
            let target = normalize_target(target)
                .ok_or_else(|| UpgradeError::InvalidVersion(target.into()))?;
            Ok(format!("{base}/releases/tags/v{target}"))
        }
    }
}

#[derive(Clone)]
pub struct ReleaseClient {
    http: reqwest::Client,
    api_base: String,
}

impl std::fmt::Debug for ReleaseClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReleaseClient")
            .field("api_base", &self.api_base)
            .finish_non_exhaustive()
    }
}

impl Default for ReleaseClient {
    fn default() -> Self {
        Self::new(RELEASE_API_BASE).expect("static release API URL is valid")
    }
}

impl ReleaseClient {
    pub fn new(api_base: impl Into<String>) -> Result<Self, UpgradeError> {
        let api_base = api_base.into();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { http, api_base })
    }

    pub async fn release(&self, target: Option<&str>) -> Result<ReleaseMetadata, UpgradeError> {
        let url = release_api_url(&self.api_base, target)?;
        let response = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", format!("opencode/{INSTALLATION_VERSION}"))
            .send()
            .await?
            .error_for_status()?;
        let release = response.json::<ReleaseMetadata>().await?;
        let actual = normalize_target(&release.tag_name)
            .ok_or_else(|| UpgradeError::UnsupportedMetadata("release tag is invalid".into()))?;
        if let Some(target) = target {
            let expected = normalize_target(target)
                .ok_or_else(|| UpgradeError::InvalidVersion(target.into()))?;
            if actual != expected {
                return Err(UpgradeError::UnsupportedMetadata(format!(
                    "release tag {actual} does not match requested {expected}"
                )));
            }
        }
        Ok(release)
    }

    pub async fn download(&self, asset: &ReleaseAsset) -> Result<Vec<u8>, UpgradeError> {
        let url = url::Url::parse(&asset.browser_download_url).map_err(|error| {
            UpgradeError::UnsupportedMetadata(format!("invalid asset URL: {error}"))
        })?;
        if url.scheme() != "https" || url.host_str().is_none() {
            return Err(UpgradeError::UnsupportedMetadata(
                "release asset URL must use HTTPS".into(),
            ));
        }
        if asset.size.unwrap_or(0) > MAX_RELEASE_BYTES {
            return Err(UpgradeError::ReleaseTooLarge);
        }
        let response = self
            .http
            .get(url)
            .header("User-Agent", format!("opencode/{INSTALLATION_VERSION}"))
            .send()
            .await?
            .error_for_status()?;
        if response.content_length().unwrap_or(0) > MAX_RELEASE_BYTES {
            return Err(UpgradeError::ReleaseTooLarge);
        }
        let bytes = response.bytes().await?;
        let actual = bytes.len() as u64;
        if actual > MAX_RELEASE_BYTES {
            return Err(UpgradeError::ReleaseTooLarge);
        }
        if let Some(expected) = asset.size.filter(|size| *size > 0) {
            if actual != expected {
                return Err(UpgradeError::ReleaseSizeMismatch { expected, actual });
            }
        }
        Ok(bytes.to_vec())
    }
}

pub fn select_asset<'a>(
    release: &'a ReleaseMetadata,
    platform: ReleasePlatform,
) -> Result<&'a ReleaseAsset, UpgradeError> {
    let expected = platform.asset_name();
    let asset = release.assets.iter().find(|asset| asset.name == expected);
    let Some(asset) = asset else {
        return Err(UpgradeError::UnsupportedMetadata(format!(
            "no compatible asset named {expected}"
        )));
    };
    let url = url::Url::parse(&asset.browser_download_url).map_err(|error| {
        UpgradeError::UnsupportedMetadata(format!("invalid asset URL: {error}"))
    })?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(UpgradeError::UnsupportedMetadata(
            "release asset URL must use HTTPS".into(),
        ));
    }
    if asset.size == Some(0) || asset.size.unwrap_or(0) > MAX_RELEASE_BYTES {
        return Err(UpgradeError::UnsupportedMetadata(
            "asset size metadata is unsupported".into(),
        ));
    }
    Ok(asset)
}

fn archive_entry_matches(path: &Path, binary_name: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == binary_name)
}

pub fn extract_binary(
    archive: &[u8],
    asset_name: &str,
    platform: ReleasePlatform,
) -> Result<Vec<u8>, UpgradeError> {
    let binary_name = platform.binary_name();
    if platform.os == ReleaseOs::Linux {
        if !asset_name.ends_with(".tar.gz") {
            return Err(UpgradeError::UnsupportedMetadata(
                "Linux release asset is not a .tar.gz archive".into(),
            ));
        }
        let decoder = flate2::read::GzDecoder::new(Cursor::new(archive));
        let mut tar = tar::Archive::new(decoder);
        let entries = tar
            .entries()
            .map_err(|error| UpgradeError::Archive(error.to_string()))?;
        for entry in entries {
            let mut entry = entry.map_err(|error| UpgradeError::Archive(error.to_string()))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry
                .path()
                .map_err(|error| UpgradeError::Archive(error.to_string()))?
                .into_owned();
            if archive_entry_matches(&path, binary_name) {
                let mut binary = Vec::new();
                entry.read_to_end(&mut binary)?;
                return Ok(binary);
            }
        }
    } else {
        if !asset_name.ends_with(".zip") {
            return Err(UpgradeError::UnsupportedMetadata(
                "non-Linux release asset is not a .zip archive".into(),
            ));
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(archive))?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() || archive_entry_matches(Path::new(entry.name()), binary_name) {
                if entry.is_dir() {
                    continue;
                }
                if entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000)
                {
                    continue;
                }
                let mut binary = Vec::new();
                entry.read_to_end(&mut binary)?;
                return Ok(binary);
            }
        }
    }
    Err(UpgradeError::BinaryNotFound(binary_name.into()))
}

fn temp_path(destination: &Path) -> Result<PathBuf, UpgradeError> {
    let parent = destination.parent().ok_or_else(|| {
        UpgradeError::ExecutableUnavailable("executable has no parent directory".into())
    })?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| UpgradeError::ExecutableUnavailable("executable has no file name".into()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(parent.join(format!(
        ".{name}.upgrade-{}-{timestamp}.tmp",
        std::process::id()
    )))
}

pub fn install_binary_atomic(destination: &Path, binary: &[u8]) -> Result<(), UpgradeError> {
    if binary.is_empty() {
        return Err(UpgradeError::UnsupportedMetadata(
            "downloaded executable is empty".into(),
        ));
    }
    let metadata = fs::symlink_metadata(destination).map_err(|error| {
        UpgradeError::ExecutableUnavailable(format!("{}: {error}", destination.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(UpgradeError::ExecutableUnavailable(format!(
            "{} is not a regular executable file",
            destination.display()
        )));
    }
    let parent = destination.parent().ok_or_else(|| {
        UpgradeError::ExecutableUnavailable("executable has no parent directory".into())
    })?;
    let temporary = temp_path(destination)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(binary)?;
        file.sync_all()?;
        #[cfg(unix)]
        fs::set_permissions(&temporary, metadata.permissions())?;
        drop(file);

        #[cfg(windows)]
        {
            return Err(UpgradeError::UnsupportedMetadata(
                "Windows cannot replace the running executable atomically; restart from a package installer"
                    .into(),
            ));
        }
        #[cfg(not(windows))]
        {
            fs::rename(&temporary, destination)?;
            let directory = fs::File::open(parent)?;
            directory.sync_all()?;
            Ok(())
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub async fn install_release(
    client: &ReleaseClient,
    target: &str,
    platform: ReleasePlatform,
    destination: &Path,
) -> Result<String, UpgradeError> {
    let target =
        normalize_target(target).ok_or_else(|| UpgradeError::InvalidVersion(target.into()))?;
    let release = client.release(Some(&target)).await?;
    let asset = select_asset(&release, platform)?;
    let archive = client.download(asset).await?;
    let binary = extract_binary(&archive, &asset.name, platform)?;
    install_binary_atomic(destination, &binary)?;
    Ok(asset.name.clone())
}

pub async fn fetch_release(target: Option<&str>) -> Result<ReleaseMetadata, UpgradeError> {
    ReleaseClient::default().release(target).await
}

/// Mirrors `Installation.getReleaseType(current, latest)`.
pub fn get_release_type(current: &str, latest: &str) -> ReleaseType {
    let current = parse_version(current).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    });
    let latest = parse_version(latest).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    });
    let curr_major = current.major;
    let curr_minor = current.minor;
    let new_major = latest.major;
    let new_minor = latest.minor;
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
    fetch_release(None)
        .await
        .ok()
        .and_then(|release| normalize_target(&release.tag_name))
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

/// Startup update behavior, matching the reference `autoupdate` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupUpdatePolicy {
    Disabled,
    Notify,
    Auto,
}

/// Installation method, mirroring `Installation.method()` in
/// `reference/packages/opencode/src/installation/index.ts`. The reference only
/// self-replaces the running binary when the method is known (`npm`, `brew`,
/// or the install script); an unknown method — including a cargo `target/`
/// development build — must never be auto-upgraded in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationMethod {
    Npm,
    Yarn,
    Pnpm,
    Bun,
    Brew,
    Scoop,
    Choco,
    /// The install script (`opencode.ai/install`), the reference's `curl`
    /// method.
    Script,
    Unknown,
}

/// Classify how the running binary was installed from its path.
///
/// This is the fast path used by startup auto-update: it must not spawn a
/// package-manager subprocess. The full command-probing detection is
/// [`detect_method`].
pub fn installation_method(exe: &std::path::Path) -> InstallationMethod {
    let text = exe.to_string_lossy();
    if text.contains("node_modules") {
        InstallationMethod::Npm
    } else if text.contains("Cellar") || text.contains("homebrew") {
        InstallationMethod::Brew
    } else if text.contains(".opencode/bin") || text.contains(".local/bin") {
        InstallationMethod::Script
    } else {
        InstallationMethod::Unknown
    }
}

/// Capture `stdout` for a subprocess, returning an empty string on any error.
///
/// Mirrors the reference `text()` helper in `installation/index.ts`, which
/// swallows failures so a missing package manager simply yields no output.
fn run_text(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// The checks probed by [`detect_method`], mirroring the reference
/// `checks` array in `installation/index.ts`.
fn method_checks() -> &'static [(&'static str, &'static [&'static str], &'static str)] {
    &[
        // (method, command args, installed package name to look for)
        ("npm", &["list", "-g", "--depth=0"], "opencode-ai"),
        ("yarn", &["global", "list"], "opencode-ai"),
        ("pnpm", &["list", "-g", "--depth=0"], "opencode-ai"),
        ("bun", &["pm", "ls", "-g"], "opencode-ai"),
        ("brew", &["list", "--formula", "opencode"], "opencode"),
        ("scoop", &["list", "opencode"], "opencode"),
        ("choco", &["list", "--limit-output", "opencode"], "opencode"),
    ]
}

/// Detect the installation method by probing each package manager, mirroring
/// `Installation.method()` in `reference/packages/opencode/src/installation/index.ts`.
///
/// The install-script (`Script`) method is recognized from the binary path
/// first; otherwise the package managers whose names appear in `exe` are tried
/// first (matching the reference sort), and the first whose `list` output
/// contains its installed package name wins. `runner` executes the probe so
/// tests can substitute a PATH stub or a recorded closure.
pub fn detect_method_with_runner(
    exe: &std::path::Path,
    mut runner: impl FnMut(&str, &[&str]) -> String,
) -> InstallationMethod {
    let text = exe.to_string_lossy();
    if text.contains(".opencode/bin") || text.contains(".local/bin") {
        return InstallationMethod::Script;
    }
    let exec = text.to_lowercase();

    // Sort so the method whose name appears in the executable path is probed
    // first (reference sort is stable, per-method).
    let mut order: Vec<usize> = (0..method_checks().len()).collect();
    order.sort_by_key(|&index| {
        let (name, ..) = method_checks()[index];
        if exec.contains(name) {
            0
        } else {
            1
        }
    });

    for index in order {
        let (name, args, installed_name) = method_checks()[index];
        let output = runner(name, args);
        if output.to_lowercase().contains(installed_name) {
            return match name {
                "npm" => InstallationMethod::Npm,
                "yarn" => InstallationMethod::Yarn,
                "pnpm" => InstallationMethod::Pnpm,
                "bun" => InstallationMethod::Bun,
                "brew" => InstallationMethod::Brew,
                "scoop" => InstallationMethod::Scoop,
                "choco" => InstallationMethod::Choco,
                _ => InstallationMethod::Unknown,
            };
        }
    }
    InstallationMethod::Unknown
}

/// Detect the installation method using the process `PATH`.
pub fn detect_method(exe: &std::path::Path) -> InstallationMethod {
    detect_method_with_runner(exe, run_text)
}

/// The package-manager command that would upgrade `target`, mirroring the
/// `upgrade()` switch in `reference/packages/opencode/src/installation/index.ts`.
///
/// Returns `Err` for methods that cannot self-upgrade through the native port
/// (the install script and `yarn`, which the reference detects but does not
/// expose an upgrade command for).
pub fn package_manager_command(
    method: InstallationMethod,
    target: &str,
) -> Result<Vec<String>, String> {
    let target = normalize_target(target).ok_or_else(|| "invalid release version".to_string())?;
    match method {
        InstallationMethod::Npm => Ok(vec![
            "npm".into(),
            "install".into(),
            "-g".into(),
            format!("opencode-ai@{target}"),
        ]),
        InstallationMethod::Pnpm => Ok(vec![
            "pnpm".into(),
            "install".into(),
            "-g".into(),
            format!("opencode-ai@{target}"),
        ]),
        InstallationMethod::Bun => Ok(vec![
            "bun".into(),
            "install".into(),
            "-g".into(),
            format!("opencode-ai@{target}"),
        ]),
        InstallationMethod::Brew => Ok(vec!["brew".into(), "upgrade".into(), "opencode".into()]),
        InstallationMethod::Choco => Ok(vec![
            "choco".into(),
            "upgrade".into(),
            "opencode".into(),
            format!("--version={target}"),
            "-y".into(),
        ]),
        InstallationMethod::Scoop => Ok(vec![
            "scoop".into(),
            "install".into(),
            format!("opencode@{target}"),
        ]),
        _ => Err(format!(
            "installation method {method:?} has no package-manager upgrade"
        )),
    }
}

/// Resolve startup update behavior from the environment and `autoupdate`
/// config value. The explicit disable flag wins; the always-notify flag then
/// opts in even when the config is `false`.
pub fn startup_update_policy(
    config: Option<&AutoUpdate>,
    disable_from_env: bool,
    always_notify_from_env: bool,
) -> StartupUpdatePolicy {
    if disable_from_env {
        return StartupUpdatePolicy::Disabled;
    }
    if always_notify_from_env {
        return StartupUpdatePolicy::Notify;
    }
    match config {
        Some(AutoUpdate::Enabled(false)) => StartupUpdatePolicy::Disabled,
        Some(AutoUpdate::Notify) => StartupUpdatePolicy::Notify,
        Some(AutoUpdate::Enabled(true)) | None => StartupUpdatePolicy::Auto,
    }
}

/// Resolve the startup policy using the process environment.
pub fn startup_update_policy_from_env(config: Option<&AutoUpdate>) -> StartupUpdatePolicy {
    startup_update_policy(config, autoupdate_disabled(), always_notify_update())
}

/// Return the notification text only when `latest` is newer than the running
/// release. Keeping this separate makes the comparison deterministic and
/// prevents malformed release metadata from producing a notification.
pub fn startup_update_message(latest: &str) -> Option<String> {
    let current = parse_version(INSTALLATION_VERSION)?;
    let latest_version = parse_version(latest)?;
    if latest_version <= current {
        return None;
    }
    let latest = normalize_target(latest)?;
    Some(format!(
        "Update available: {INSTALLATION_VERSION} → {latest}. Run `opencode upgrade` to update."
    ))
}

/// The action selected after a startup release check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupUpdateAction {
    None,
    Notify(String),
    Install(String),
}

/// An `opencode upgrade` outcome derived entirely from the current and
/// requested versions. Mirrors the guard logic in the explicit
/// `cli/cmd/upgrade.ts` path and `upgrade_cmd.rs` (F039 "refuses downgrades").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeDecision {
    /// The requested target equals the installed version: skip.
    AlreadyInstalled,
    /// The requested target is older than the installed version: refuse.
    RefusesDowngrade,
    /// The request is valid and newer than the installed version.
    Proceed,
}

/// Decide whether an explicit `upgrade <target>` should proceed.
///
/// Returns `None` when the installed `current` version cannot be parsed, so
/// the caller can refuse before attempting anything (matching the invalid
/// installed-version refusals in `upgrade_cmd.rs`).
pub fn upgrade_decision(current: &str, requested: &str) -> Option<UpgradeDecision> {
    let current = parse_version(current)?;
    let requested = parse_version(requested)?;
    if requested == current {
        Some(UpgradeDecision::AlreadyInstalled)
    } else if requested < current {
        Some(UpgradeDecision::RefusesDowngrade)
    } else {
        Some(UpgradeDecision::Proceed)
    }
}

/// Decide what startup should do for a fetched release.
///
/// `fetch` is injected so policy behavior can be tested without contacting
/// GitHub. The production wrapper below supplies `fetch_latest`.
pub async fn startup_update_action_with_fetch<F, Fut>(
    policy: StartupUpdatePolicy,
    fetch: F,
) -> StartupUpdateAction
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<String>>,
{
    if policy == StartupUpdatePolicy::Disabled {
        return StartupUpdateAction::None;
    }

    let Some(latest) = fetch().await else {
        return StartupUpdateAction::None;
    };
    let Some(message) = startup_update_message(&latest) else {
        return StartupUpdateAction::None;
    };
    let Some(latest) = normalize_target(&latest) else {
        return StartupUpdateAction::None;
    };

    if policy == StartupUpdatePolicy::Auto
        && get_release_type(INSTALLATION_VERSION, &latest) == ReleaseType::Patch
    {
        StartupUpdateAction::Install(latest)
    } else {
        StartupUpdateAction::Notify(message)
    }
}

/// Best-effort update check for the interactive default command.
///
/// Startup must remain usable offline, so config loading and network errors
/// are intentionally ignored and the release check is bounded to two seconds.
/// Patch auto-updates are handed to a background task; major/minor updates are
/// surfaced before the TUI starts. The caller controls which commands are
/// eligible; server and noninteractive commands do not call this function.
pub async fn notify_startup_update() {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return;
    }

    let config = oc_config::load::load_global(None)
        .ok()
        .and_then(|config| config.autoupdate);

    let policy = startup_update_policy_from_env(config.as_ref());
    let action = match tokio::time::timeout(
        STARTUP_UPDATE_TIMEOUT,
        startup_update_action_with_fetch(policy, fetch_latest),
    )
    .await
    {
        Ok(action) => action,
        Err(_) => return,
    };

    match action {
        StartupUpdateAction::None => {}
        StartupUpdateAction::Notify(message) => crate::cli::ui::println(&[
            crate::cli::ui::Style::TEXT_INFO_BOLD,
            "↗  ",
            crate::cli::ui::Style::TEXT_NORMAL,
            &message,
        ]),
        StartupUpdateAction::Install(target) => {
            let Ok(destination) = std::env::current_exe() else {
                return;
            };
            // The reference refuses to upgrade an unknown installation method
            // (`if (method === "unknown") return` in cli/upgrade.ts). A cargo
            // `target/` development build is unknown: replacing it would swap
            // the local Rust binary for a downloaded release and poison every
            // subsequent local test run.
            if installation_method(&destination) == InstallationMethod::Unknown {
                return;
            }
            let Ok(platform) = current_platform() else {
                return;
            };
            tokio::spawn(async move {
                let _ = install_release(&ReleaseClient::default(), &target, platform, &destination)
                    .await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_release_tags_and_rejects_arbitrary_input() {
        assert_eq!(normalize_target("v1.18.13"), Some("1.18.13".into()));
        assert_eq!(
            normalize_target(" 1.18.13+build.4 "),
            Some("1.18.13".into())
        );
        assert_eq!(normalize_target("https://example.invalid/install"), None);
        assert_eq!(normalize_target("1.18"), None);
    }

    #[test]
    fn compares_release_versions() {
        assert!(parse_version("1.19.0") > parse_version("1.18.13"));
        assert!(parse_version("2.0.0") > parse_version("1.99.99"));
        assert_eq!(get_release_type("1.18.13", "2.0.0"), ReleaseType::Major);
        assert_eq!(get_release_type("1.18.13", "1.19.0"), ReleaseType::Minor);
        assert_eq!(get_release_type("1.18.13", "1.18.14"), ReleaseType::Patch);
    }

    #[test]
    fn upgrade_decision_refuses_downgrades_without_network() {
        use super::UpgradeDecision as D;
        // Same version -> skip.
        assert_eq!(
            upgrade_decision("1.18.13", "v1.18.13"),
            Some(D::AlreadyInstalled)
        );
        // Newer target -> proceed.
        assert_eq!(upgrade_decision("1.18.13", "1.18.14"), Some(D::Proceed));
        assert_eq!(upgrade_decision("1.18.13", "2.0.0"), Some(D::Proceed));
        // Older target -> explicit downgrade refusal (F039).
        assert_eq!(
            upgrade_decision("1.18.13", "1.18.12"),
            Some(D::RefusesDowngrade)
        );
        assert_eq!(
            upgrade_decision("1.18.13", "1.0.0"),
            Some(D::RefusesDowngrade)
        );
        // An unparseable installed version -> None so the caller can refuse.
        assert_eq!(upgrade_decision("dev", "1.18.14"), None);
    }

    #[test]
    fn startup_policy_honors_config_and_environment_precedence() {
        assert_eq!(
            startup_update_policy(Some(&AutoUpdate::Enabled(false)), false, false),
            StartupUpdatePolicy::Disabled
        );
        assert_eq!(
            startup_update_policy(Some(&AutoUpdate::Notify), false, false),
            StartupUpdatePolicy::Notify
        );
        assert_eq!(
            startup_update_policy(None, false, false),
            StartupUpdatePolicy::Auto
        );
        assert_eq!(
            startup_update_policy(Some(&AutoUpdate::Enabled(true)), true, true),
            StartupUpdatePolicy::Disabled
        );
        assert_eq!(
            startup_update_policy(Some(&AutoUpdate::Enabled(false)), false, true),
            StartupUpdatePolicy::Notify
        );
    }

    #[tokio::test]
    async fn startup_action_uses_injected_fetch_without_network() {
        assert_eq!(
            startup_update_action_with_fetch(StartupUpdatePolicy::Disabled, || async {
                panic!("disabled startup update must not fetch")
            })
            .await,
            StartupUpdateAction::None
        );
        assert_eq!(
            startup_update_action_with_fetch(StartupUpdatePolicy::Auto, || async {
                Some("v1.18.14".to_string())
            })
            .await,
            StartupUpdateAction::Install("1.18.14".into())
        );
        assert!(matches!(
            startup_update_action_with_fetch(StartupUpdatePolicy::Auto, || async {
                Some("1.19.0".to_string())
            })
            .await,
            StartupUpdateAction::Notify(_)
        ));
        assert!(matches!(
            startup_update_action_with_fetch(StartupUpdatePolicy::Notify, || async {
                Some("1.18.14".to_string())
            })
            .await,
            StartupUpdateAction::Notify(_)
        ));
        assert_eq!(
            startup_update_action_with_fetch(StartupUpdatePolicy::Auto, || async { None }).await,
            StartupUpdateAction::None
        );
    }

    #[test]
    fn startup_notification_is_only_emitted_for_a_newer_release() {
        assert!(startup_update_message(INSTALLATION_VERSION).is_none());
        assert!(startup_update_message("1.18.12").is_none());
        assert!(startup_update_message("not-a-version").is_none());
        assert_eq!(
            startup_update_message("1.18.14").as_deref(),
            Some("Update available: 1.18.13 → 1.18.14. Run `opencode upgrade` to update.")
        );
    }

    #[test]
    fn release_urls_and_asset_selection_are_strict() {
        assert_eq!(
            release_api_url("https://example.test/api", Some("v1.2.3")).unwrap(),
            "https://example.test/api/releases/tags/v1.2.3"
        );
        assert!(release_api_url("https://example.test/api", Some("latest")).is_err());

        let platform = ReleasePlatform {
            os: ReleaseOs::Linux,
            arch: ReleaseArch::X64,
            musl: false,
            baseline: false,
        };
        let release = ReleaseMetadata {
            tag_name: "v1.2.3".into(),
            assets: vec![ReleaseAsset {
                name: platform.asset_name(),
                browser_download_url: "https://example.test/opencode.tar.gz".into(),
                size: Some(12),
            }],
        };
        assert_eq!(select_asset(&release, platform).unwrap().size, Some(12));
    }

    #[cfg(not(windows))]
    #[test]
    fn atomic_install_preserves_executable_mode_and_replaces_contents() {
        let root = std::env::temp_dir().join(format!(
            "opencode-upgrade-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("opencode");
        fs::write(&destination, b"old").unwrap();
        install_binary_atomic(&destination, b"new").unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn cargo_target_builds_are_unknown_installation_method() {
        // The reference refuses to self-upgrade unknown installation methods;
        // a cargo dev build must classify as unknown so startup auto-install
        // never replaces target/debug/opencode with a downloaded release.
        assert_eq!(
            installation_method(std::path::Path::new("/repo/target/debug/opencode")),
            super::InstallationMethod::Unknown
        );
        assert_eq!(
            installation_method(std::path::Path::new("/repo/target/release/opencode")),
            super::InstallationMethod::Unknown
        );
    }

    #[test]
    fn known_installation_methods_are_detected() {
        assert_eq!(
            installation_method(std::path::Path::new(
                "/nvm/versions/node/v22/lib/node_modules/@opencode-ai/cli/bin/opencode2.exe"
            )),
            super::InstallationMethod::Npm
        );
        assert_eq!(
            installation_method(std::path::Path::new(
                "/opt/homebrew/Cellar/opencode/1.0/bin/opencode"
            )),
            super::InstallationMethod::Brew
        );
        assert_eq!(
            installation_method(std::path::Path::new("/home/u/.opencode/bin/opencode")),
            super::InstallationMethod::Script
        );
        assert_eq!(
            installation_method(std::path::Path::new("/home/u/.local/bin/opencode")),
            super::InstallationMethod::Script
        );
    }

    #[test]
    fn detect_method_probes_package_managers_via_injected_runner() {
        use super::InstallationMethod as M;

        // No package manager lists opencode -> unknown.
        let detected =
            detect_method_with_runner(std::path::Path::new("/usr/bin/opencode"), |_, _| {
                "nothing".to_string()
            });
        assert_eq!(detected, M::Unknown);

        // npm lists opencode-ai -> npm.
        let detected =
            detect_method_with_runner(std::path::Path::new("/usr/bin/opencode"), |prog, args| {
                assert_eq!(prog, "npm");
                assert_eq!(args, ["list", "-g", "--depth=0"]);
                "openai opencode-ai ara  ".to_string()
            });
        assert_eq!(detected, M::Npm);

        // brew lists opencode -> brew (checks npm first, which outputs nothing).
        let detected =
            detect_method_with_runner(std::path::Path::new("/usr/bin/opencode"), |prog, _| {
                if prog == "brew" {
                    "opencode".to_string()
                } else {
                    String::new()
                }
            });
        assert_eq!(detected, M::Brew);

        // choco lists opencode -> choco.
        let detected =
            detect_method_with_runner(std::path::Path::new("/usr/bin/opencode"), |prog, _| {
                if prog == "choco" {
                    "opencode".to_string()
                } else {
                    String::new()
                }
            });
        assert_eq!(detected, M::Choco);

        // scoop lists opencode -> scoop.
        let detected =
            detect_method_with_runner(std::path::Path::new("/usr/bin/opencode"), |prog, _| {
                if prog == "scoop" {
                    "opencode".to_string()
                } else {
                    String::new()
                }
            });
        assert_eq!(detected, M::Scoop);

        // The install-script path wins immediately without probing.
        let mut probed = false;
        let detected = detect_method_with_runner(
            std::path::Path::new("/home/u/.opencode/bin/opencode"),
            |_, _| {
                probed = true;
                String::new()
            },
        );
        assert_eq!(detected, M::Script);
        assert!(!probed, "install-script path must not spawn a probe");
    }

    #[test]
    fn detect_method_prefers_package_manager_named_in_exec_path() {
        use super::InstallationMethod as M;
        // Both npm and brew outputs match, but the exec path names `brew`,
        // so brew is probed first and wins on a tie.
        let detected = detect_method_with_runner(
            std::path::Path::new("/usr/local/brew/bin/opencode"),
            |prog, _| {
                if prog == "brew" {
                    "opencode".to_string()
                } else if prog == "npm" {
                    "opencode-ai".to_string()
                } else {
                    String::new()
                }
            },
        );
        assert_eq!(detected, M::Brew);
    }

    #[test]
    fn detect_method_finds_package_managers_on_path() {
        use super::InstallationMethod as M;
        // A PATH stub for `npm` that lists opencode-ai; the real subprocess
        // runner resolves it from the injected PATH.
        let root = std::env::temp_dir().join(format!(
            "oc-upgrade-method-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let stub = root.join("npm");
        fs::write(&stub, "#!/bin/sh\nprintf 'schedules opencode-ai\\n'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let previous = std::env::var_os("PATH");
        let path = format!(
            "{}:{}",
            root.display(),
            previous.as_deref().unwrap_or_default().to_string_lossy()
        );
        std::env::set_var("PATH", &path);
        let detected = detect_method(std::path::Path::new("/usr/bin/opencode"));
        std::env::remove_var("PATH");
        if let Some(previous) = previous {
            std::env::set_var("PATH", previous);
        }
        let _ = fs::remove_dir_all(root);
        assert_eq!(detected, M::Npm);
    }

    #[test]
    fn package_manager_command_dispatches_per_method() {
        use super::InstallationMethod as M;
        assert_eq!(
            package_manager_command(M::Npm, "1.2.3").unwrap(),
            vec!["npm", "install", "-g", "opencode-ai@1.2.3"]
        );
        assert_eq!(
            package_manager_command(M::Pnpm, "v1.2.3").unwrap(),
            vec!["pnpm", "install", "-g", "opencode-ai@1.2.3"]
        );
        assert_eq!(
            package_manager_command(M::Bun, "1.2.3").unwrap(),
            vec!["bun", "install", "-g", "opencode-ai@1.2.3"]
        );
        assert_eq!(
            package_manager_command(M::Choco, "1.2.3").unwrap(),
            vec!["choco", "upgrade", "opencode", "--version=1.2.3", "-y"]
        );
        assert_eq!(
            package_manager_command(M::Scoop, "1.2.3").unwrap(),
            vec!["scoop", "install", "opencode@1.2.3"]
        );
        assert_eq!(
            package_manager_command(M::Brew, "1.2.3").unwrap(),
            vec!["brew", "upgrade", "opencode"]
        );
        assert!(package_manager_command(M::Yarn, "1.2.3").is_err());
        assert!(package_manager_command(M::Script, "1.2.3").is_err());
        assert!(package_manager_command(M::Unknown, "1.2.3").is_err());
    }
}
