//! Shared helpers.

use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

/// A boxed future alias for object-safe async trait methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Equivalent of `withTimeout` in
/// `reference/packages/opencode/src/util/timeout.ts`.
pub async fn with_timeout<T, F>(future: F, ms: u64, label: &str) -> crate::Result<T>
where
    F: std::future::Future<Output = crate::Result<T>>,
{
    let timeout = tokio::time::sleep(Duration::from_millis(ms));
    tokio::pin!(timeout);
    tokio::select! {
        result = future => result,
        _ = &mut timeout => Err(crate::Error::Timeout { ms, label: label.to_string() }),
    }
}

/// `Date.now() / 1000` — seconds since the Unix epoch (used for OAuth expiry).
pub fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

/// `pathToFileURL(directory).href` from Node, for the `roots/list` handler.
pub fn path_to_file_url(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let encoded: String = canonical
        .to_string_lossy()
        .chars()
        .map(|c| match c {
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            '?' => "%3F".to_string(),
            ' ' => "%20".to_string(),
            other => other.to_string(),
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_file_url_absolute() {
        assert_eq!(
            path_to_file_url(Path::new("/tmp/foo bar")),
            "file:///tmp/foo%20bar"
        );
        assert_eq!(
            path_to_file_url(Path::new("/home/user/proj")),
            "file:///home/user/proj"
        );
    }
}
