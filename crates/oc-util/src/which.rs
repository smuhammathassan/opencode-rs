/// From reference/packages/core/src/util/which.ts
///
/// Resolves an executable on `PATH`, appending `Global.Path.bin` (mirroring
/// the `which` npm package's `path` option). On Windows, `PATHEXT` extensions
/// are tried.
use std::path::PathBuf;

pub fn which(cmd: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
    dirs.push(crate::global::path::bin());

    for dir in dirs {
        let candidate = dir.join(cmd);
        if let Ok(metadata) = std::fs::metadata(&candidate) {
            if metadata.is_file() && is_executable(&candidate, &metadata) {
                return Some(candidate);
            }
        }
        #[cfg(windows)]
        {
            let pathext =
                std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
            for ext in pathext.split(';').filter(|e| !e.is_empty()) {
                let with_ext = dir.join(format!("{cmd}{ext}"));
                if with_ext.is_file() {
                    return Some(with_ext);
                }
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(_path: &PathBuf, metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_path: &PathBuf, _metadata: &std::fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::which;

    #[test]
    fn finds_commands_on_path() {
        assert!(which("sh").is_some());
        assert!(which("definitely-not-a-real-command-xyz").is_none());
    }
}
