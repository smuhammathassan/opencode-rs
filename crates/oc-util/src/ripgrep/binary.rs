/// From reference/packages/core/src/ripgrep/binary.ts
///
/// Resolves the ripgrep binary: first the system `rg`, then the cached
/// `Global.Path.bin/rg`, finally downloading the pinned release
/// (`ripgrep-15.1.0`) and extracting it. The reference extracts tar.gz with
/// `tar` and zip with PowerShell; this port mirrors that.
use std::fmt;
use std::path::PathBuf;

use tokio::sync::OnceCell;

const VERSION: &str = "15.1.0";

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

fn platform_config() -> Option<(&'static str, &'static str)> {
    if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        return Some(("aarch64-apple-darwin", "tar.gz"));
    }
    if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        return Some(("aarch64-unknown-linux-gnu", "tar.gz"));
    }
    if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        return Some(("x86_64-apple-darwin", "tar.gz"));
    }
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        return Some(("x86_64-unknown-linux-musl", "tar.gz"));
    }
    if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
        return Some(("aarch64-pc-windows-msvc", "zip"));
    }
    if cfg!(all(target_arch = "x86", target_os = "windows")) {
        return Some(("i686-pc-windows-msvc", "zip"));
    }
    if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
        return Some(("x86_64-pc-windows-msvc", "zip"));
    }
    None
}

fn rg_exe() -> &'static str {
    if cfg!(windows) {
        "rg.exe"
    } else {
        "rg"
    }
}

pub async fn filepath() -> Result<PathBuf, Error> {
    static CACHE: OnceCell<PathBuf> = OnceCell::const_new();
    let path = CACHE.get_or_try_init(resolve_filepath).await?;
    Ok(path.clone())
}

/// Serialize first-time resolution so concurrent `oc-tool` callers do not race
/// the same download/extract in the shared `Global.Path.bin` directory.
static SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static SYNC_CACHE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Synchronous variant for callers that spawn `rg` from non-async tool paths
/// (e.g. `oc-tool`'s glob/grep). Resolves PATH → cached binary → download,
/// mirroring `filepath()` but through the blocking reqwest client. The
/// blocking client is created on a dedicated thread so it is never dropped
/// inside a tokio async context (which panics with "Cannot drop a runtime in
/// a context where blocking is not allowed").
pub fn filepath_sync() -> Result<PathBuf, Error> {
    if let Some(path) = SYNC_CACHE.get() {
        return Ok(path.clone());
    }
    let _guard = SYNC_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(path) = SYNC_CACHE.get() {
        return Ok(path.clone());
    }

    if let Some(system) = crate::which::which(rg_exe()) {
        if std::path::Path::new(&system).is_file() {
            let _ = SYNC_CACHE.set(system.clone());
            return Ok(system);
        }
    }

    let bin_dir = crate::global::path::bin();
    let target = bin_dir.join(rg_exe());
    if target.is_file() {
        let _ = SYNC_CACHE.set(target.clone());
        return Ok(target);
    }

    let handle = std::thread::spawn(resolve_blocking);
    let result = handle
        .join()
        .unwrap_or_else(|_| Err(Error("ripgrep resolution thread panicked".into())));
    if let Ok(path) = &result {
        let _ = SYNC_CACHE.set(path.clone());
    }
    result
}

/// Thread-isolated blocking resolution: re-checks PATH and the cached binary
/// (cheap), then downloads the pinned release with `reqwest::blocking` and
/// extracts it into `Global.Path.bin`.
fn resolve_blocking() -> Result<PathBuf, Error> {
    let bin_dir = crate::global::path::bin();
    let target = bin_dir.join(rg_exe());

    let Some((platform, extension)) = platform_config() else {
        return Err(Error(format!(
            "unsupported platform for ripgrep: {}",
            std::env::consts::OS
        )));
    };
    let filename = format!("ripgrep-{VERSION}-{platform}.{extension}");
    let url =
        format!("https://github.com/BurntSushi/ripgrep/releases/download/{VERSION}/{filename}");
    let archive = bin_dir.join(&filename);

    tracing::info!("downloading ripgrep: {url}");
    std::fs::create_dir_all(&bin_dir)?;
    let response = reqwest::blocking::get(&url)
        .map_err(|e| Error(format!("failed to download ripgrep: {e}")))?;
    let bytes = response
        .bytes()
        .map_err(|e| Error(format!("failed to download ripgrep: {e}")))?;
    if bytes.is_empty() {
        return Err(Error(format!("failed to download ripgrep from {url}")));
    }
    std::fs::write(&archive, &bytes)?;

    if let Err(e) = extract_blocking(&archive, &bin_dir, platform, extension) {
        let _ = std::fs::remove_file(&archive);
        return Err(e);
    }
    let _ = std::fs::remove_file(&archive);
    Ok(target)
}

async fn resolve_filepath() -> Result<PathBuf, Error> {
    if let Some(system) = crate::which::which(rg_exe()) {
        if crate::fs_util::is_file(&system.to_string_lossy()).await {
            return Ok(system);
        }
    }

    let bin_dir = crate::global::path::bin();
    let target = bin_dir.join(rg_exe());
    if crate::fs_util::is_file(&target.to_string_lossy()).await {
        return Ok(target);
    }

    let Some((platform, extension)) = platform_config() else {
        return Err(Error(format!(
            "unsupported platform for ripgrep: {}",
            std::env::consts::OS
        )));
    };
    let filename = format!("ripgrep-{VERSION}-{platform}.{extension}");
    let url =
        format!("https://github.com/BurntSushi/ripgrep/releases/download/{VERSION}/{filename}");
    let archive = bin_dir.join(&filename);

    tracing::info!("downloading ripgrep: {url}");
    crate::fs_util::ensure_dir(&bin_dir.to_string_lossy())
        .await
        .map_err(|e| Error(e.to_string()))?;
    let response = reqwest::get(&url)
        .await
        .map_err(|e| Error(format!("failed to download ripgrep: {e}")))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| Error(format!("failed to download ripgrep: {e}")))?;
    if bytes.is_empty() {
        return Err(Error(format!("failed to download ripgrep from {url}")));
    }
    crate::fs_util::write_with_dirs(&archive.to_string_lossy(), bytes.to_vec(), None)
        .await
        .map_err(|e| Error(e.to_string()))?;

    extract(&archive, &bin_dir, platform, extension).await?;
    let _ = std::fs::remove_file(&archive);
    Ok(target)
}

async fn extract(
    archive: &std::path::Path,
    bin_dir: &std::path::Path,
    platform: &str,
    extension: &str,
) -> Result<(), Error> {
    use crate::util::process::{run, RunOptions};

    if extension == "zip" {
        #[cfg(windows)]
        {
            let cmd = format!(
                "$global:ProgressPreference = 'SilentlyContinue'; Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                archive.to_string_lossy().replace('\'', "''"),
                bin_dir.to_string_lossy().replace('\'', "''")
            );
            let out = run(
                &[
                    "powershell.exe".to_string(),
                    "-NoProfile".to_string(),
                    "-NonInteractive".to_string(),
                    "-Command".to_string(),
                    cmd,
                ],
                &RunOptions {
                    nothrow: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_else(|_| crate::util::process::Result {
                code: 1,
                stdout: Vec::new(),
                stderr: Vec::new(),
            });
            if out.code != 0 {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let message = if !stderr.is_empty() {
                    stderr
                } else if !stdout.is_empty() {
                    stdout
                } else {
                    format!("ripgrep extraction failed with code {}", out.code)
                };
                return Err(Error(message));
            }
        }
    }

    if extension == "tar.gz" {
        let out = run(
            &[
                "tar".to_string(),
                "-xzf".to_string(),
                archive.to_string_lossy().into_owned(),
                "-C".to_string(),
                bin_dir.to_string_lossy().into_owned(),
            ],
            &RunOptions {
                nothrow: true,
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|_| crate::util::process::Result {
            code: 1,
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
        if out.code != 0 {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let message = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("ripgrep extraction failed with code {}", out.code)
            };
            return Err(Error(message));
        }
    }

    let extracted = bin_dir
        .join(format!("ripgrep-{VERSION}-{platform}"))
        .join(rg_exe());
    if !crate::fs_util::is_file(&extracted.to_string_lossy()).await {
        return Err(Error(format!(
            "ripgrep archive did not contain executable: {}",
            extracted.to_string_lossy()
        )));
    }
    std::fs::copy(&extracted, bin_dir.join(rg_exe()))?;
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(bin_dir.join(rg_exe()))?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(bin_dir.join(rg_exe()), permissions)?;
    }
    Ok(())
}

/// Blocking extraction for the synchronous resolver. Mirrors `extract()` but
/// runs the archive via `std::process::Command` so no async runtime is needed.
fn extract_blocking(
    archive: &std::path::Path,
    bin_dir: &std::path::Path,
    platform: &str,
    extension: &str,
) -> Result<(), Error> {
    if extension == "zip" {
        #[cfg(windows)]
        {
            let cmd = format!(
                "$global:ProgressPreference = 'SilentlyContinue'; Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                archive.to_string_lossy().replace('\'', "''"),
                bin_dir.to_string_lossy().replace('\'', "''")
            );
            let output = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", cmd.as_str()])
                .output();
            let out = match output {
                Ok(out) => out,
                Err(_) => {
                    return Err(Error(
                        "ripgrep extraction failed to spawn powershell".into(),
                    ))
                }
            };
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return Err(Error(if !stderr.is_empty() {
                    stderr
                } else if !stdout.is_empty() {
                    stdout
                } else {
                    format!("ripgrep extraction failed with code {}", out.status)
                }));
            }
        }
    }

    if extension == "tar.gz" {
        let output = std::process::Command::new("tar")
            .args([
                "-xzf".as_ref(),
                archive.to_string_lossy().as_ref(),
                "-C".as_ref(),
                bin_dir.to_string_lossy().as_ref(),
            ])
            .output();
        let out = match output {
            Ok(out) => out,
            Err(_) => return Err(Error("ripgrep extraction failed to spawn tar".into())),
        };
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Err(Error(if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("ripgrep extraction failed with code {}", out.status)
            }));
        }
    }

    let extracted = bin_dir
        .join(format!("ripgrep-{VERSION}-{platform}"))
        .join(rg_exe());
    if !extracted.is_file() {
        return Err(Error(format!(
            "ripgrep archive did not contain executable: {}",
            extracted.to_string_lossy()
        )));
    }
    std::fs::copy(&extracted, bin_dir.join(rg_exe()))?;
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(bin_dir.join(rg_exe()))?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(bin_dir.join(rg_exe()), permissions)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_an_executable() {
        let path = filepath().await.unwrap();
        assert!(crate::fs_util::is_file(&path.to_string_lossy()).await);
    }

    #[test]
    fn platform_mapping_covers_current_host() {
        assert!(platform_config().is_some());
    }
}
