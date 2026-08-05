/// From reference/packages/opencode/src/util/timeout.ts
///
/// Races a future against a timeout, mirroring `Promise.race` in
/// `withTimeout`. Inner errors are preserved; a timeout produces the same
/// message the reference does: `label ?? \`Operation timed out after ${ms}ms\``.
use std::future::Future;
use std::time::Duration;

#[derive(Debug)]
pub enum WithTimeoutError<E> {
    TimedOut(String),
    Inner(E),
}

impl<E: std::fmt::Display> std::fmt::Display for WithTimeoutError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WithTimeoutError::TimedOut(message) => write!(f, "{message}"),
            WithTimeoutError::Inner(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for WithTimeoutError<E> {}

pub async fn with_timeout<F, T, E>(
    future: F,
    ms: u64,
    label: Option<&str>,
) -> Result<T, WithTimeoutError<E>>
where
    F: Future<Output = Result<T, E>>,
{
    match tokio::time::timeout(Duration::from_millis(ms), future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(WithTimeoutError::Inner(error)),
        Err(_) => Err(WithTimeoutError::TimedOut(match label {
            Some(label) => label.to_string(),
            None => format!("Operation timed out after {ms}ms"),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_before_timeout() {
        let result = with_timeout(async { Ok::<_, ()>(42) }, 1000, None).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn times_out() {
        let result = with_timeout::<_, _, ()>(
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(1)
            },
            10,
            None,
        )
        .await;
        match result {
            Err(WithTimeoutError::TimedOut(message)) => {
                assert_eq!(message, "Operation timed out after 10ms")
            }
            _ => panic!("expected timeout"),
        }
    }

    #[tokio::test]
    async fn uses_label() {
        let result = with_timeout::<_, _, ()>(
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(1)
            },
            10,
            Some("my label"),
        )
        .await;
        match result {
            Err(WithTimeoutError::TimedOut(message)) => assert_eq!(message, "my label"),
            _ => panic!("expected timeout"),
        }
    }

    #[tokio::test]
    async fn preserves_inner_error() {
        let result = with_timeout(async { Err::<u8, _>("boom") }, 1000, None).await;
        match result {
            Err(WithTimeoutError::Inner(error)) => assert_eq!(error, "boom"),
            _ => panic!("expected inner error"),
        }
    }
}
