//! General-purpose rate limiting for the dashboard API and GitHub webhook
//! endpoint. `check_auth_rate_limit` in `auth.rs` predates this and stays
//! separate (keyed by IP+email, not IP/session) — this module covers
//! everything else so we don't hand-roll a second limiter per call site.
//!
//! Per-process, in-memory, fixed-window. Same tradeoffs as the login limiter:
//! not shared across replicas, resets on restart. Fine for single-node /
//! Compose deploys; put a shared limiter in front for multi-instance prod.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use super::errors::ApiError;
use super::AppState;

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Requests per minute per session/IP for authenticated dashboard/API routes.
fn api_rate_limit_per_min() -> u32 {
    env_u32("CODASAURUS_API_RATE_LIMIT_PER_MIN", 100)
}

/// Requests per minute per source IP for the GitHub webhook receiver.
/// GitHub can burst deliveries (e.g. bulk re-runs, org-wide pushes), so this
/// is deliberately much higher than the dashboard API limit.
fn webhook_rate_limit_per_min() -> u32 {
    env_u32("CODASAURUS_WEBHOOK_RATE_LIMIT_PER_MIN", 300)
}

pub struct RateLimiter {
    limit: u32,
    window: Duration,
    hits: Mutex<HashMap<String, (u32, Instant)>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Returns `Err(retry_after)` once `key` has exceeded its limit for the
    /// current window.
    pub fn check(&self, key: &str) -> Result<(), Duration> {
        let mut map = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        map.retain(|_, (_, started)| now.duration_since(*started) < self.window);
        let entry = map.entry(key.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) >= self.window {
            *entry = (0, now);
        }
        if entry.0 >= self.limit {
            let retry_after = self.window.saturating_sub(now.duration_since(entry.1));
            return Err(retry_after);
        }
        entry.0 += 1;
        Ok(())
    }
}

static API_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(api_rate_limit_per_min(), Duration::from_secs(60)));

static WEBHOOK_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(webhook_rate_limit_per_min(), Duration::from_secs(60)));

/// Same IP-extraction logic as the login rate limiter: trusted reverse proxy
/// `X-Forwarded-For` (first hop), then `X-Real-Ip`, else "unknown".
pub(crate) fn client_ip(headers: &HeaderMap) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            let t = first.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    if let Some(rip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let t = rip.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    "unknown".into()
}

fn too_many(retry_after: Duration, msg: &str) -> ApiError {
    ApiError::too_many_requests_after(msg, retry_after.as_secs().max(1))
}

/// Axum middleware for authenticated REST/dashboard routes. Keyed by session
/// cookie when present (so one user's tabs share a budget), falling back to
/// client IP for anything unauthenticated that still reaches this layer.
pub async fn api_rate_limit_middleware(
    State(_state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let key = super::auth::extract_token(req.headers()).unwrap_or_else(|| client_ip(req.headers()));
    API_LIMITER
        .check(&key)
        .map_err(|retry_after| too_many(retry_after, "Too many requests. Please slow down."))?;
    Ok(next.run(req).await)
}

/// Called directly from the webhook handler (it's a bare `post()` route with
/// no state/middleware stack — see `serve.rs`).
pub(crate) fn check_webhook_rate_limit(headers: &HeaderMap) -> Result<(), ApiError> {
    WEBHOOK_LIMITER
        .check(&client_ip(headers))
        .map_err(|retry_after| too_many(retry_after, "Too many webhook deliveries."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_limit_then_resets_after_window() {
        let limiter = RateLimiter::new(2, Duration::from_millis(50));
        assert!(limiter.check("k").is_ok());
        assert!(limiter.check("k").is_ok());
        assert!(limiter.check("k").is_err());

        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.check("k").is_ok());
    }

    #[test]
    fn keys_are_independent() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        assert!(limiter.check("a").is_ok());
        assert!(limiter.check("b").is_ok());
        assert!(limiter.check("a").is_err());
    }
}
