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
        // Add simple jitter (±25%) without requiring the rand crate.
        let jitter = (ms / 4).saturating_mul((attempt as u64 % 3) + 1) / 3;
        Duration::from_millis((ms + jitter).min(30_000))
    }
}

/// Retry an async fallible operation with exponential backoff.
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
                    tracing::warn!(
                        operation,
                        attempt = attempt + 1,
                        ?delay,
                        error = %e,
                        "retrying after failure"
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("retry exhausted for {operation}")))
}

/// Predicate: is this an error we should retry?
pub fn is_reqwest_error_retryable(err: &anyhow::Error) -> bool {
    if let Some(req_err) = err.downcast_ref::<reqwest::Error>() {
        if req_err.is_timeout() || req_err.is_connect() {
            return true;
        }
        if let Some(status) = req_err.status() {
            return status.is_server_error() || status.as_u16() == 429;
        }
        return true;
    }
    // Also match our RateLimited / HttpStatus wrappers
    let msg = err.to_string();
    msg.contains("429") || msg.contains("rate limit") || msg.contains("503") || msg.contains("502")
}

/// Send a GitHub API request with status-aware retries (429 / 5xx) and Retry-After support.
pub async fn github_request(
    config: &RetryConfig,
    operation: &str,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> anyhow::Result<reqwest::Response> {
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        let resp = match build().send().await {
            Ok(r) => r,
            Err(e) => {
                let err: anyhow::Error = e.into();
                if attempt < config.max_retries && is_reqwest_error_retryable(&err) {
                    let delay = config.delay(attempt);
                    tracing::warn!(operation, attempt = attempt + 1, ?delay, error = %err, "github transport retry");
                    tokio::time::sleep(delay).await;
                    last_err = Some(err);
                    continue;
                }
                return Err(err);
            }
        };

        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }

        let retryable = status.as_u16() == 429 || status.is_server_error();
        if status.as_u16() == 429 {
            crate::metrics::record_github_429();
        }
        if attempt < config.max_retries && retryable {
            crate::metrics::record_github_retry();
            let delay = parse_retry_after(&resp).unwrap_or_else(|| config.delay(attempt));
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(
                operation,
                attempt = attempt + 1,
                %status,
                ?delay,
                "github status retry"
            );
            tokio::time::sleep(delay).await;
            last_err = Some(anyhow::anyhow!(
                "GitHub API {operation} returned {status}: {body}"
            ));
            continue;
        }

        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "GitHub API {operation} returned {status}: {body}"
        ));
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("retry exhausted for {operation}")))
}

fn parse_retry_after(resp: &reqwest::Response) -> Option<Duration> {
    if let Some(v) = resp
        .headers()
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
    {
        if let Ok(secs) = v.parse::<u64>() {
            return Some(Duration::from_secs(secs.min(60)));
        }
    }
    if let Some(v) = resp
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if v > now {
            return Some(Duration::from_secs((v - now).min(60)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_grows_and_caps() {
        let cfg = RetryConfig::api_default();
        assert!(cfg.delay(0).as_millis() >= 1000);
        assert!(cfg.delay(10).as_millis() <= 30_000);
    }
}
