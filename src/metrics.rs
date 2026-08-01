//! In-process Prometheus-style metrics for reviews, GitHub, and LLM.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static REVIEWS_TOTAL: AtomicU64 = AtomicU64::new(0);
static REVIEWS_FAILED: AtomicU64 = AtomicU64::new(0);
static REVIEWS_TIMED_OUT: AtomicU64 = AtomicU64::new(0);
static GITHUB_429: AtomicU64 = AtomicU64::new(0);
static GITHUB_RETRIES: AtomicU64 = AtomicU64::new(0);
static LLM_REQUESTS: AtomicU64 = AtomicU64::new(0);
static LLM_ERRORS: AtomicU64 = AtomicU64::new(0);
static LLM_PROMPT_CHARS: AtomicU64 = AtomicU64::new(0);
static QUEUE_ENQUEUED: AtomicU64 = AtomicU64::new(0);
static QUEUE_COMPLETED: AtomicU64 = AtomicU64::new(0);

/// Rolling latency samples (milliseconds) for approximate p50/p95.
static LATENCIES_MS: OnceLock<Mutex<VecDeque<u64>>> = OnceLock::new();
const LATENCY_CAP: usize = 512;

fn latencies() -> &'static Mutex<VecDeque<u64>> {
    LATENCIES_MS.get_or_init(|| Mutex::new(VecDeque::with_capacity(LATENCY_CAP)))
}

pub fn record_review_ok(started: Instant) {
    REVIEWS_TOTAL.fetch_add(1, Ordering::Relaxed);
    let ms = started.elapsed().as_millis() as u64;
    if let Ok(mut q) = latencies().lock() {
        if q.len() >= LATENCY_CAP {
            q.pop_front();
        }
        q.push_back(ms);
    }
}

pub fn record_review_failed() {
    REVIEWS_FAILED.fetch_add(1, Ordering::Relaxed);
}

pub fn record_review_timeout() {
    REVIEWS_TIMED_OUT.fetch_add(1, Ordering::Relaxed);
}

pub fn record_github_429() {
    GITHUB_429.fetch_add(1, Ordering::Relaxed);
}

pub fn record_github_retry() {
    GITHUB_RETRIES.fetch_add(1, Ordering::Relaxed);
}

pub fn record_llm_request(prompt_chars: usize) {
    LLM_REQUESTS.fetch_add(1, Ordering::Relaxed);
    LLM_PROMPT_CHARS.fetch_add(prompt_chars as u64, Ordering::Relaxed);
}

pub fn record_llm_error() {
    LLM_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_queue_enqueued() {
    QUEUE_ENQUEUED.fetch_add(1, Ordering::Relaxed);
}

pub fn record_queue_completed() {
    QUEUE_COMPLETED.fetch_add(1, Ordering::Relaxed);
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Render Prometheus text exposition format.
pub fn render_prometheus() -> String {
    let mut samples: Vec<u64> = latencies()
        .lock()
        .map(|q| q.iter().copied().collect())
        .unwrap_or_default();
    samples.sort_unstable();
    let p50 = percentile(&samples, 50.0);
    let p95 = percentile(&samples, 95.0);
    let count = samples.len() as u64;

    format!(
        "# HELP codasaurus_up Codasaurus process is up\n\
         # TYPE codasaurus_up gauge\n\
         codasaurus_up 1\n\
         # HELP codasaurus_build_info Build version\n\
         # TYPE codasaurus_build_info gauge\n\
         codasaurus_build_info{{version=\"{}\"}} 1\n\
         # HELP codasaurus_reviews_total Completed reviews\n\
         # TYPE codasaurus_reviews_total counter\n\
         codasaurus_reviews_total {}\n\
         # HELP codasaurus_reviews_failed_total Failed reviews\n\
         # TYPE codasaurus_reviews_failed_total counter\n\
         codasaurus_reviews_failed_total {}\n\
         # HELP codasaurus_reviews_timed_out_total Timed out reviews\n\
         # TYPE codasaurus_reviews_timed_out_total counter\n\
         codasaurus_reviews_timed_out_total {}\n\
         # HELP codasaurus_review_latency_ms Review latency percentiles\n\
         # TYPE codasaurus_review_latency_ms gauge\n\
         codasaurus_review_latency_ms{{quantile=\"0.5\"}} {p50}\n\
         codasaurus_review_latency_ms{{quantile=\"0.95\"}} {p95}\n\
         # HELP codasaurus_review_latency_samples Latency sample count in window\n\
         # TYPE codasaurus_review_latency_samples gauge\n\
         codasaurus_review_latency_samples {count}\n\
         # HELP codasaurus_github_429_total GitHub HTTP 429 responses\n\
         # TYPE codasaurus_github_429_total counter\n\
         codasaurus_github_429_total {}\n\
         # HELP codasaurus_github_retries_total GitHub retry attempts\n\
         # TYPE codasaurus_github_retries_total counter\n\
         codasaurus_github_retries_total {}\n\
         # HELP codasaurus_llm_requests_total LLM API requests\n\
         # TYPE codasaurus_llm_requests_total counter\n\
         codasaurus_llm_requests_total {}\n\
         # HELP codasaurus_llm_errors_total LLM API errors\n\
         # TYPE codasaurus_llm_errors_total counter\n\
         codasaurus_llm_errors_total {}\n\
         # HELP codasaurus_llm_prompt_chars_total Approximate prompt characters sent\n\
         # TYPE codasaurus_llm_prompt_chars_total counter\n\
         codasaurus_llm_prompt_chars_total {}\n\
         # HELP codasaurus_queue_enqueued_total Review jobs enqueued\n\
         # TYPE codasaurus_queue_enqueued_total counter\n\
         codasaurus_queue_enqueued_total {}\n\
         # HELP codasaurus_queue_completed_total Review jobs completed\n\
         # TYPE codasaurus_queue_completed_total counter\n\
         codasaurus_queue_completed_total {}\n",
        env!("CARGO_PKG_VERSION"),
        REVIEWS_TOTAL.load(Ordering::Relaxed),
        REVIEWS_FAILED.load(Ordering::Relaxed),
        REVIEWS_TIMED_OUT.load(Ordering::Relaxed),
        GITHUB_429.load(Ordering::Relaxed),
        GITHUB_RETRIES.load(Ordering::Relaxed),
        LLM_REQUESTS.load(Ordering::Relaxed),
        LLM_ERRORS.load(Ordering::Relaxed),
        LLM_PROMPT_CHARS.load(Ordering::Relaxed),
        QUEUE_ENQUEUED.load(Ordering::Relaxed),
        QUEUE_COMPLETED.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_empty_and_basic() {
        assert_eq!(percentile(&[], 50.0), 0);
        assert_eq!(percentile(&[10, 20, 30, 40, 50], 50.0), 30);
    }

    #[test]
    fn render_includes_core_series() {
        let body = render_prometheus();
        assert!(body.contains("codasaurus_up 1"));
        assert!(body.contains("codasaurus_reviews_total"));
        assert!(body.contains("codasaurus_github_429_total"));
    }
}
