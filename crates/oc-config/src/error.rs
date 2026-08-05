// From reference/packages/core/src/v1/config/error.ts

/// A single validation issue carried by `Invalid`.
///
/// The reference model is `{ message: string, path: string[], ...rest }`, so
/// additional keys like `code` and `keys` survive round-trips.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Issue {
    pub message: String,
    pub path: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
}

impl Issue {
    pub fn new(message: impl Into<String>, path: impl Into<Vec<String>>) -> Self {
        Self {
            message: message.into(),
            path: path.into(),
            code: None,
            keys: None,
        }
    }
}

/// Config load/validation errors, mirroring the `NamedError` classes in
/// `reference/packages/core/src/v1/config/error.ts`.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigError {
    /// `ConfigJsonError`: the config text is not valid JSON(C).
    Json { path: String, message: Option<String> },
    /// `ConfigInvalidError`: the config failed schema validation.
    Invalid {
        path: String,
        issues: Vec<Issue>,
        message: Option<String>,
    },
    /// `ConfigFrontmatterError`: a markdown config file has bad YAML frontmatter.
    Frontmatter { path: String, message: String },
    /// `ConfigDirectoryTypoError`: a `.{mode,command,agent,plugin}` directory is misspelled.
    DirectoryTypo {
        path: String,
        dir: String,
        suggestion: String,
    },
    /// `ConfigRemoteAuthError`: a remote config URL answered with a login page.
    RemoteAuth { url: String, remote: String },
    /// I/O failures while reading or writing config files.
    Io { path: String, error: String },
}

impl ConfigError {
    pub fn json(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Json {
            path: path.into(),
            message: Some(message.into()),
        }
    }

    pub fn invalid(path: impl Into<String>, issues: Vec<Issue>, message: Option<String>) -> Self {
        Self::Invalid {
            path: path.into(),
            issues,
            message,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Json { path, .. }
            | Self::Invalid { path, .. }
            | Self::Frontmatter { path, .. }
            | Self::DirectoryTypo { path, .. }
            | Self::Io { path, .. } => path,
            Self::RemoteAuth { remote, .. } => remote,
        }
    }

    /// Formats the error the same way `FormatError` does in
    /// `reference/packages/opencode/src/cli/error.ts`.
    pub fn format(&self) -> String {
        match self {
            Self::Json { path, message } => {
                let message = message.clone().unwrap_or_default();
                format!(
                    "Config file at {path} is not valid JSON(C){}{}",
                    if message.is_empty() { "" } else { ": " },
                    message
                )
            }
            Self::Invalid {
                path,
                issues,
                message,
            } => {
                let mut out = format!(
                    "Configuration is invalid{}",
                    if path.is_empty() || path == "config" {
                        String::new()
                    } else {
                        format!(" at {path}")
                    }
                );
                if let Some(message) = message {
                    if !message.is_empty() {
                        out.push_str(": ");
                        out.push_str(message);
                    }
                }
                for issue in issues {
                    out.push('\n');
                    out.push_str("↳ ");
                    out.push_str(&issue.message);
                    if !issue.path.is_empty() {
                        out.push(' ');
                        out.push_str(&issue.path.join("."));
                    }
                }
                out
            }
            Self::Frontmatter { message, .. } => message.clone(),
            Self::DirectoryTypo {
                dir, path, suggestion, ..
            } => format!(
                "Directory \"{dir}\" in {path} is not valid. Rename the directory to \"{suggestion}\" or remove it. This is a common typo."
            ),
            Self::RemoteAuth { url, remote } => format!(
                "Failed to load remote config{}: the server returned a login page instead of JSON.\nAuthentication is missing or has expired (the endpoint is likely behind an SSO or identity-aware proxy).{}",
                if remote.is_empty() {
                    String::new()
                } else {
                    format!(" from {remote}")
                },
                if url.is_empty() {
                    String::new()
                } else {
                    format!("\nRun `opencode auth login {url}` to re-authenticate.")
                }
            ),
            Self::Io { path, error } => format!("Failed to read config file at {path}: {error}"),
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format())
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            path: String::new(),
            error: error.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, ConfigError>;
