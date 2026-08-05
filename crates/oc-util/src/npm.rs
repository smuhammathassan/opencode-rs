/// From reference/packages/core/src/npm.ts
///
/// Only the `which` path is ported: resolve a package's bin from the shared
/// package cache. The reference installs with `@npmcli/arborist`; here
/// `TODO(integration): replace the npm-CLI install with an in-process
/// arborist-equivalent`.
use std::path::PathBuf;

pub fn sanitize(pkg: &str) -> String {
    #[cfg(windows)]
    {
        let illegal = ['<', '>', ':', '"', '|', '?', '*'];
        pkg.chars()
            .map(|c| {
                if illegal.contains(&c) || (c as u32) < 32 {
                    '_'
                } else {
                    c
                }
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        pkg.to_string()
    }
}

fn unscoped(pkg: &str) -> &str {
    if pkg.starts_with('@') {
        pkg.split('/').nth(1).unwrap_or(pkg)
    } else {
        pkg
    }
}

fn pick(
    dir: &std::path::Path,
    bin_dir: &std::path::Path,
    pkg: &str,
    bin: Option<&str>,
) -> Option<String> {
    let mut files: Vec<String> = std::fs::read_dir(bin_dir)
        .ok()?
        .filter_map(|entry| {
            entry
                .ok()
                .map(|e| e.file_name().to_string_lossy().into_owned())
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return None;
    }
    if let Some(bin) = bin {
        if files.contains(&bin.to_string()) {
            return Some(bin_dir.join(bin).to_string_lossy().into_owned());
        }
        return None;
    }
    if files.len() == 1 {
        return Some(bin_dir.join(&files[0]).to_string_lossy().into_owned());
    }

    let pkg_json_path = dir.join("node_modules").join(pkg).join("package.json");
    if let Ok(text) = std::fs::read_to_string(&pkg_json_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            match json.get("bin") {
                Some(serde_json::Value::String(_)) => {
                    return Some(bin_dir.join(unscoped(pkg)).to_string_lossy().into_owned())
                }
                Some(serde_json::Value::Object(map)) => {
                    let keys: Vec<&String> = map.keys().collect();
                    let name = if keys.len() == 1 {
                        keys[0].clone()
                    } else if map.contains_key(unscoped(pkg)) {
                        unscoped(pkg).to_string()
                    } else {
                        keys[0].clone()
                    };
                    return Some(bin_dir.join(name).to_string_lossy().into_owned());
                }
                _ => {}
            }
        }
    }
    Some(bin_dir.join(&files[0]).to_string_lossy().into_owned())
}

/// Resolves `pkg`'s bin (default: the single bin exposed by the package),
/// installing it into the shared package cache if missing.
pub async fn which(pkg: &str, bin: Option<&str>) -> Option<String> {
    let dir = crate::global::path::cache()
        .join("packages")
        .join(sanitize(pkg));
    let bin_dir = dir.join("node_modules").join(".bin");
    if let Some(resolved) = pick(&dir, &bin_dir, pkg, bin) {
        return Some(resolved);
    }
    install(pkg, &dir).await;
    pick(&dir, &bin_dir, pkg, bin)
}

async fn install(pkg: &str, dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    let args = vec![
        "install".to_string(),
        "--no-save".to_string(),
        "--no-audit".to_string(),
        "--no-fund".to_string(),
        "--prefix".to_string(),
        dir.to_string_lossy().into_owned(),
        pkg.to_string(),
    ];
    let _ = crate::util::process::run(
        &args,
        &crate::util::process::RunOptions {
            nothrow: true,
            ..Default::default()
        },
    )
    .await;
}

/// The cache directory used for a package (mirrors `Npm.add`'s `directory`).
pub fn package_dir(pkg: &str) -> PathBuf {
    crate::global::path::cache()
        .join("packages")
        .join(sanitize(pkg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_leaves_unix_names_alone() {
        assert_eq!(sanitize("prettier"), "prettier");
        assert_eq!(sanitize("@biomejs/biome"), "@biomejs/biome");
    }

    #[test]
    fn unscoped_handles_scoped_packages() {
        assert_eq!(unscoped("@biomejs/biome"), "biome");
        assert_eq!(unscoped("prettier"), "prettier");
    }

    #[tokio::test]
    async fn which_returns_none_for_missing_package() {
        let result = which("definitely-not-a-real-package-xyz", None).await;
        assert!(result.is_none());
    }
}
