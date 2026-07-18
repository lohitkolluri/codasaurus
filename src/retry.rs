use std::future::Future;
use std::time::Duration;

/// Configuration for retry behaviour.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
        }
    }
}

impl RetryConfig {
    /// Liberal retry budget for network-intensive API calls.
    pub const fn api_default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
        }
    }

    /// Conservative retry budget for quick idempotent lookups.
    pub const fn quick() -> Self {
        Self {
            max_retries: 2,
            base_delay_ms: 500,
        }
    }

    fn delay(&self, attempt: u32) -> Duration {
        let ms = self.base_delay_ms * (1u64 << attempt);
        Duration::from_millis(ms.min(30_000)) // cap at 30 s
    }
}

/// Retry an async fallible operation with exponential backoff.
///
/// `operation` is a human-readable label used in warning messages.
/// `is_retryable` is called with the error to decide whether to retry.
/// `f` is the closure to retry — called fresh on each attempt.
pub async fn retry_async<F, Fut, P, T>(
    config: &RetryConfig,
    operation: &str,
    is_retryable: P,
    f: F,
) -> anyhow::Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
    P: Fn(&anyhow::Error) -> bool + Send + Sync,
{
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt < config.max_retries && is_retryable(&e) {
                    let delay = config.delay(attempt);
                    eprintln!(
                        "Warning: {} failed (attempt {}), retrying in {:?}: {}",
                        operation,
                        attempt + 1,
                        delay,
                        e
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("retry exhausted for {}", operation)))
}

/// Retry a blocking fallible operation with exponential backoff.
pub fn retry_blocking<F, T>(
    config: &RetryConfig,
    operation: &str,
    is_retryable: &dyn Fn(&anyhow::Error) -> bool,
    f: F,
) -> anyhow::Result<T>
where
    F: Fn() -> anyhow::Result<T>,
{
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        match f() {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt < config.max_retries && is_retryable(&e) {
                    let delay = config.delay(attempt);
                    eprintln!(
                        "Warning: {} failed (attempt {}), retrying in {:?}: {}",
                        operation,
                        attempt + 1,
                        delay,
                        e
                    );
                    std::thread::sleep(delay);
                    last_err = Some(e);
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("retry exhausted for {}", operation)))
}

/// Predicate: is this an error we should retry?
///
/// Matches transient failures: network errors (timeout, connect, DNS),
/// server errors (5xx), and rate-limiting (429).
///
/// Note: both `reqwest::Client` (async) and `reqwest::blocking::Client` use
/// the same `reqwest::Error` type, so a single downcast covers both.
pub fn is_reqwest_error_retryable(err: &anyhow::Error) -> bool {
    if let Some(req_err) = err.downcast_ref::<reqwest::Error>() {
        if req_err.is_timeout() || req_err.is_connect() {
            return true;
        }
        if let Some(status) = req_err.status() {
            return status.is_server_error() || status.as_u16() == 429;
        }
        // Transport-level error without a status (connection reset, DNS failure, etc.)
        return true;
    }
    false
}
