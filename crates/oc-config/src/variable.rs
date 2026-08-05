// From reference/packages/opencode/src/config/variable.ts

use crate::error::{ConfigError, Result};
use indexmap::IndexMap;
use regex::Regex;

/// How a missing `{file:...}` reference is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Missing {
    #[default]
    Error,
    Empty,
}

/// The config source the substitution is relative to.
#[derive(Debug, Clone)]
pub enum Source {
    /// A file on disk; `{file:...}` resolves from its directory.
    Path { path: String },
    /// Virtual content; `{file:...}` resolves from `dir`.
    Virtual { source: String, dir: String },
}

impl Source {
    pub(crate) fn display(&self) -> &str {
        match self {
            Source::Path { path } => path,
            Source::Virtual { source, .. } => source,
        }
    }

    fn dir(&self) -> String {
        match self {
            Source::Path { path } => parent(path),
            Source::Virtual { dir, .. } => dir.clone(),
        }
    }
}

fn parent(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => normalized[..i].to_string(),
        None => ".".to_string(),
    }
}

/// Applies `{env:VAR}` and `{file:path}` substitutions to config text.
///
/// Mirror of `ConfigVariable.substitute`: environment references are replaced
/// first, then file references are resolved relative to the config directory.
/// A `{file:...}` token on a `//`-commented line is left untouched.
pub fn substitute(
    text: &str,
    source: &Source,
    env: Option<&IndexMap<String, String>>,
    missing: Missing,
) -> Result<String> {
    let env_re = Regex::new(r"\{env:([^}]+)\}").expect("static regex");
    let substituted = env_re.replace_all(text, |captures: &regex::Captures<'_>| {
        let var = &captures[1];
        env.and_then(|e| e.get(var).cloned())
            .or_else(|| std::env::var(var).ok())
            .unwrap_or_default()
    });

    let file_re = Regex::new(r"\{file:[^}]+\}").expect("static regex");
    let matches: Vec<_> = file_re.find_iter(&substituted).collect();
    if matches.is_empty() {
        return Ok(substituted.into_owned());
    }

    let config_dir = source.dir();
    let config_source = source.display().to_string();
    let mut out = String::new();
    let mut cursor = 0;

    for matched in matches {
        let token = matched.as_str();
        let index = matched.start();
        out.push_str(&substituted[cursor..index]);

        let line_start = substituted[..index].rfind('\n').map_or(0, |i| i + 1);
        let prefix = substituted[line_start..index].trim_start();
        if prefix.starts_with("//") {
            out.push_str(token);
            cursor = index + token.len();
            continue;
        }

        let file_path = token
            .strip_prefix("{file:")
            .and_then(|t| t.strip_suffix('}'))
            .unwrap_or(token);
        let resolved = if let Some(rest) = file_path.strip_prefix("~/") {
            home_dir().join(rest).to_string_lossy().into_owned()
        } else if is_absolute(file_path) {
            file_path.to_string()
        } else {
            join(&config_dir, file_path)
        };

        let content = match read_text(&resolved) {
            Ok(content) => content,
            Err(_error) if missing == Missing::Empty => String::new(),
            Err(error) => {
                let message = if error.kind() == std::io::ErrorKind::NotFound {
                    format!("bad file reference: \"{token}\" {resolved} does not exist")
                } else {
                    format!("bad file reference: \"{token}\"")
                };
                return Err(ConfigError::invalid(
                    config_source,
                    Vec::new(),
                    Some(message),
                ));
            }
        };

        let escaped = serde_json::to_string(&content.trim()).map_err(|e| {
            ConfigError::invalid(config_source.clone(), Vec::new(), Some(e.to_string()))
        })?;
        out.push_str(escaped.trim_start_matches('"').trim_end_matches('"'));
        cursor = index + token.len();
    }

    out.push_str(&substituted[cursor..]);
    Ok(out)
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('\\') || path.starts_with("file://")
}

fn join(dir: &str, path: &str) -> String {
    if dir.is_empty() || dir == "." {
        path.to_string()
    } else {
        format!(
            "{}/{}",
            dir.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

fn home_dir() -> std::path::PathBuf {
    crate::paths::home_dir()
}

fn read_text(path: &str) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}
