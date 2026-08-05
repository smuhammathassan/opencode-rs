//! Port of `reference/packages/core/src/shell.ts` — shell selection helpers.

const META_DENIED: [&str; 2] = ["fish", "nu"];

/// `Shell.name` from `reference/packages/core/src/shell.ts`.
pub fn name(file: &str) -> String {
    std::path::Path::new(file)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| file.to_lowercase())
}

/// `Shell.ps` from `reference/packages/core/src/shell.ts`.
pub fn ps(file: &str) -> bool {
    matches!(name(file).as_str(), "powershell" | "pwsh")
}

fn which(cmd: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn stat_file(file: &str) -> Option<()> {
    std::fs::symlink_metadata(file).ok().map(|_| ())
}

fn resolve(file: &str) -> Option<String> {
    if std::path::Path::new(file).is_absolute() {
        if stat_file(file).is_some() {
            return Some(file.to_string());
        }
        return None;
    }
    which(file)
}

/// `Shell.select(..., { acceptable: true })` — `Shell.acceptable` from
/// `reference/packages/core/src/shell.ts`.
pub fn acceptable(configured: &Option<String>) -> String {
    if let Some(file) = configured {
        if !META_DENIED.contains(&name(file).as_str()) {
            if let Some(resolved) = resolve(file) {
                return resolved;
            }
        }
    }
    fallback()
}

fn fallback() -> String {
    if cfg!(target_os = "macos") {
        return "/bin/zsh".to_string();
    }
    which("bash").unwrap_or_else(|| "/bin/sh".to_string())
}
