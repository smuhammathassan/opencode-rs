//! Server authentication (HTTP Basic). From reference/packages/server/src/auth.ts and
//! reference/packages/opencode/src/server/auth.ts.

use std::env;

/// Decoded Basic credentials.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedCredentials {
    pub username: String,
    pub password: String,
}

/// Server auth configuration.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub username: String,
    pub password: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        // Username defaults to "opencode" like the reference Config layer.
        AuthConfig {
            username: "opencode".into(),
            password: None,
        }
    }
}

impl AuthConfig {
    /// From reference/packages/server/src/auth.ts (`Config.layer`): reads
    /// `OPENCODE_SERVER_PASSWORD` and `OPENCODE_SERVER_USERNAME` (default "opencode").
    pub fn from_env() -> Self {
        AuthConfig {
            username: env::var("OPENCODE_SERVER_USERNAME").unwrap_or_else(|_| "opencode".into()),
            password: env::var("OPENCODE_SERVER_PASSWORD")
                .ok()
                .filter(|p| !p.is_empty()),
        }
    }

    /// Whether auth is enforced at all.
    pub fn required(&self) -> bool {
        self.password.as_ref().is_some_and(|p| !p.is_empty())
    }

    /// Constant-time credential comparison.
    pub fn authorized(&self, credentials: &DecodedCredentials) -> bool {
        let Some(password) = self.password.as_ref() else {
            return false;
        };
        credentials.username == self.username && subtle_equal(&credentials.password, password)
    }
}

/// Build a `Authorization: Basic ...` header value. From
/// reference/packages/server/src/auth.ts (`header`).
pub fn basic_header(username: &str, password: &str) -> String {
    let raw = format!("{username}:{password}");
    let encoded =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, raw.as_bytes());
    format!("Basic {encoded}")
}

/// Decode a Basic `Authorization` header value. From
/// reference/packages/server/src/middleware/authorization.ts (`decodeCredential`).
pub fn decode_credential(input: &str) -> DecodedCredentials {
    let Ok(decoded) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, input)
    else {
        return DecodedCredentials {
            username: String::new(),
            password: String::new(),
        };
    };
    let Ok(header) = String::from_utf8(decoded) else {
        return DecodedCredentials {
            username: String::new(),
            password: String::new(),
        };
    };
    let Some((username, password)) = header.split_once(':') else {
        return DecodedCredentials {
            username: String::new(),
            password: String::new(),
        };
    };
    DecodedCredentials {
        username: username.to_string(),
        password: password.to_string(),
    }
}

/// Extract credentials from a request. From reference/packages/server/src/middleware/
/// authorization.ts (`credentialFromRequest`): checks `auth_token` query first, then the
/// `Authorization` header.
pub fn credentials_from_request(
    query_token: Option<&str>,
    authorization: Option<&str>,
) -> DecodedCredentials {
    if let Some(token) = query_token {
        return decode_credential(token);
    }
    let Some(authorization) = authorization else {
        return DecodedCredentials {
            username: String::new(),
            password: String::new(),
        };
    };
    let Some(encoded) = strip_basic(authorization) else {
        return DecodedCredentials {
            username: String::new(),
            password: String::new(),
        };
    };
    decode_credential(encoded)
}

fn strip_basic(header: &str) -> Option<&str> {
    let rest = header
        .strip_prefix("Basic ")
        .or_else(|| header.strip_prefix("basic "))?;
    Some(rest.trim())
}

/// `WWW-Authenticate` value. From reference/packages/server/src/middleware/authorization.ts.
pub const WWW_AUTHENTICATE: &str = r#"Basic realm="Secure Area""#;

pub const AUTH_TOKEN_QUERY: &str = "auth_token";

fn subtle_equal(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_header_roundtrip() {
        let header = basic_header("opencode", "secret");
        assert_eq!(header, "Basic b3BlbmNvZGU6c2VjcmV0");
        assert_eq!(
            decode_credential("b3BlbmNvZGU6c2VjcmV0"),
            DecodedCredentials {
                username: "opencode".into(),
                password: "secret".into()
            }
        );
    }

    #[test]
    fn malformed_credentials_decode_empty() {
        assert_eq!(
            decode_credential("!!!"),
            DecodedCredentials {
                username: String::new(),
                password: String::new()
            }
        );
        assert_eq!(
            decode_credential("b3BlbmNvZGU="),
            DecodedCredentials {
                username: String::new(),
                password: String::new()
            }
        );
    }

    #[test]
    fn authorized_matches_exact_credentials() {
        let config = AuthConfig {
            username: "opencode".into(),
            password: Some("secret".into()),
        };
        assert!(config.required());
        assert!(config.authorized(&DecodedCredentials {
            username: "opencode".into(),
            password: "secret".into()
        }));
        assert!(!config.authorized(&DecodedCredentials {
            username: "admin".into(),
            password: "secret".into()
        }));
        assert!(!config.authorized(&DecodedCredentials {
            username: "opencode".into(),
            password: "wrong".into()
        }));
    }

    #[test]
    fn empty_password_disables_auth() {
        let config = AuthConfig {
            username: "opencode".into(),
            password: None,
        };
        assert!(!config.required());
        assert!(!config.authorized(&DecodedCredentials {
            username: "opencode".into(),
            password: "secret".into()
        }));
    }

    #[test]
    fn credentials_from_request_prefers_query_token() {
        let from_query =
            credentials_from_request(Some("b3BlbmNvZGU6c2VjcmV0"), Some("Basic d3Jvbmc6d3Jvbmc="));
        assert_eq!(from_query.password, "secret");
        let from_header = credentials_from_request(None, Some("Basic b3BlbmNvZGU6c2VjcmV0"));
        assert_eq!(from_header.password, "secret");
    }
}
