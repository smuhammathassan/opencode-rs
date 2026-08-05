//! Path formatting helpers.
//! From reference/packages/tui/src/context/path-format.tsx (`formatPath`),
//! `util/path.ts` (`normalizePath`) and `runtime.tsx` (`abbreviateHome`).

use std::path::Path;

/// Format a path relative to `base`, falling back to an abbreviated home path.
/// From reference/packages/tui/src/context/path-format.tsx (`formatPath`)
pub fn format_path(input: &str, base: &str, home: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let absolute = if Path::new(input).is_absolute() {
        input.to_string()
    } else {
        Path::new(base).join(input).to_string_lossy().to_string()
    };
    let relative = pathdiff(Path::new(base), Path::new(&absolute));
    match relative {
        Some(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Some(rel) => {
            let rel_str = rel.to_string_lossy().to_string();
            if rel_str == ".." || rel_str.starts_with("../") || rel_str.starts_with("..\\") {
                abbreviate_home(&absolute, home)
            } else {
                rel_str
            }
        }
        None => abbreviate_home(&absolute, home),
    }
}

/// `path.relative` equivalent.
fn pathdiff(base: &Path, target: &Path) -> Option<Box<Path>> {
    let base_comps: Vec<_> = base.components().collect();
    let target_comps: Vec<_> = target.components().collect();
    let mut common = 0;
    while common < base_comps.len()
        && common < target_comps.len()
        && base_comps[common] == target_comps[common]
    {
        common += 1;
    }
    let mut result = std::path::PathBuf::new();
    for _ in common..base_comps.len() {
        result.push("..");
    }
    for comp in &target_comps[common..] {
        result.push(comp.as_os_str());
    }
    Some(result.into_boxed_path())
}

/// `~`-abbreviate paths under the home directory.
/// From reference/packages/tui/src/runtime.tsx (`abbreviateHome`)
pub fn abbreviate_home(input: &str, home: &str) -> String {
    if home.is_empty() {
        return input.to_string();
    }
    let relative = pathdiff(Path::new(home), Path::new(input));
    let relative = relative.map(|p| p.to_string_lossy().to_string());
    match relative {
        Some(rel) if rel.is_empty() => "~".to_string(),
        Some(rel)
            if rel != ".."
                && !rel.starts_with("../")
                && !rel.starts_with("..\\")
                && !Path::new(&rel).is_absolute() =>
        {
            format!("~/{rel}")
        }
        _ => input.to_string(),
    }
}

/// `normalizePath` for non-win32 platforms is the identity function.
/// From reference/packages/tui/src/util/path.ts (`normalizePath`)
pub fn normalize_path(input: &str, platform: &str) -> String {
    if platform != "win32" {
        input.to_string()
    } else {
        input.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_relative() {
        assert_eq!(
            format_path("/proj/src/a.ts", "/proj", "/home/u"),
            "src/a.ts"
        );
        assert_eq!(format_path("/proj", "/proj", "/home/u"), ".");
        assert_eq!(format_path("a.ts", "/proj/src", "/home/u"), "a.ts");
    }

    #[test]
    fn outside_base_abbreviates_home() {
        assert_eq!(
            format_path("/home/u/other/x", "/proj", "/home/u"),
            "~/other/x"
        );
        assert_eq!(format_path("/home/u", "/proj", "/home/u"), "~");
        assert_eq!(
            format_path("/etc/passwd", "/proj", "/home/u"),
            "/etc/passwd"
        );
    }

    #[test]
    fn abbreviate_home_cases() {
        assert_eq!(abbreviate_home("/home/u", "/home/u"), "~");
        assert_eq!(abbreviate_home("/home/u/.config", "/home/u"), "~/.config");
        assert_eq!(abbreviate_home("/opt/x", "/home/u"), "/opt/x");
    }
}
