// From reference/packages/opencode/src/config/entry-name.ts

/// Strips a known prefix from an already-relative path, then removes the file
/// extension. Used to derive agent/command names from `.md` file paths.
pub fn config_entry_name_from_path(relative_path: &str, prefixes: &[&str]) -> String {
    let normalized = relative_path.replace('\\', "/");
    let candidate = prefixes
        .iter()
        .find_map(|prefix| normalized.strip_prefix(prefix))
        .unwrap_or_else(|| normalized.rsplit('/').next().unwrap_or(&normalized));
    let ext = node_extname(candidate);
    if ext.is_empty() {
        candidate.to_string()
    } else {
        candidate[..candidate.len() - ext.len()].to_string()
    }
}

/// Node `path.extname` semantics: the last dot in the final segment, empty for
/// dotfiles.
fn node_extname(path: &str) -> &str {
    match path.rfind('.') {
        Some(i) if i > 0 => {
            let before = &path[..i];
            if before.ends_with('/') {
                ""
            } else {
                &path[i..]
            }
        }
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::config_entry_name_from_path;

    #[test]
    fn strips_prefixes_and_extensions() {
        assert_eq!(
            config_entry_name_from_path("agent/foo.md", &["agent/", "agents/"]),
            "foo"
        );
        assert_eq!(
            config_entry_name_from_path("agents/nested/bar.md", &["agent/", "agents/"]),
            "nested/bar"
        );
        assert_eq!(
            config_entry_name_from_path("mode/baz.md", &["mode/", "modes/"]),
            "baz"
        );
        assert_eq!(
            config_entry_name_from_path("plain.md", &["agent/", "agents/"]),
            "plain"
        );
        assert_eq!(
            config_entry_name_from_path("no/prefix/at/all.md", &["agent/", "agents/"]),
            "all"
        );
    }
}
