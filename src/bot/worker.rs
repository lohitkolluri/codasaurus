//! Durable review-queue worker.

use std::time::Duration;
use tokio::time::timeout;

use super::auth::get_installation_token;
use super::queue;
use super::review::{fetch_pull_request, review_pr_with_options, ReviewOptions};
use super::{bot_db_pool, pr_lock, prune_pr_lock, BotConfig, WebhookPayload, REVIEW_PERMITS};
use crate::bot_runtime::BotRuntimeConfig;

/// Wake the durable queue worker when a job is enqueued.
pub(crate) static QUEUE_NOTIFY: std::sync::LazyLock<tokio::sync::Notify> =
    std::sync::LazyLock::new(tokio::sync::Notify::new);

/// Background worker that drains durable review_jobs (crash recovery).
pub fn start_review_worker(pool: crate::db::DbPool, bot_cfg: BotConfig) {
    tokio::spawn(async move {
        tracing::info!("review queue worker started");
        let mut idle_ticks: u64 = 0;
        loop {
            match queue::claim_next(&pool, 600).await {
                Ok(Some(job)) => {
                    idle_ticks = 0;
                    let cfg = bot_cfg.clone();
                    let timeout_secs = BotRuntimeConfig::default().review_timeout_secs;
                    process_queued_review(
                        &cfg,
                        job.id,
                        &job.repo,
                        job.pr_number,
                        &job.head_sha,
                        job.installation_id,
                        &job.action,
                        job.attempts,
                        timeout_secs,
                    )
                    .await;
                }
                Ok(None) => {
                    idle_ticks = idle_ticks.saturating_add(1);
                    // ~every 60s of idle (2s poll × 30)
                    if idle_ticks % 30 == 0 {
                        crate::bot::maintenance::run_periodic_cleanup(&pool).await;
                    }
                    tokio::select! {
                        _ = QUEUE_NOTIFY.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "queue claim failed");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn process_queued_review(
    cfg: &BotConfig,
    job_id: i64,
    repo: &str,
    pr_number: i64,
    head_sha: &str,
    inst_id: Option<i64>,
    action: &str,
    attempts: i64,
    timeout_secs: u64,
) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };

    let pool = bot_db_pool();
    let lock = pr_lock(repo, pr_number).await;
    let _guard = lock.lock().await;
    let started = std::time::Instant::now();

    let opts = ReviewOptions {
        auto_describe: matches!(action, "opened" | "reopened" | "ready_for_review"),
        auto_review_diff: true,
    };

    let result = timeout(Duration::from_secs(timeout_secs), async {
        let token = get_installation_token(cfg, inst_id).await?;
        let pr_data = fetch_pull_request(&token, repo, pr_number).await?;
        let wrapped = WebhookPayload {
            action: String::new(),
            pull_request: Some(pr_data),
            repo: None,
            installation: None,
            comment: None,
            issue: None,
            repositories: None,
            repositories_added: None,
        };
        review_pr_with_options(&token, repo, &wrapped, opts).await
    })
    .await;

    match result {
        Ok(Ok(())) => {
            crate::metrics::record_review_ok(started);
            tracing::info!(job_id, repo, pr = pr_number, "queued review completed");
            if let Some(pool) = pool {
                let _ = queue::mark_done(pool, job_id).await;
            }
        }
        Ok(Err(e)) => {
            crate::metrics::record_review_failed();
            tracing::error!(job_id, error = %e, "queued review failed");
            release_claim_best_effort(repo, pr_number, head_sha).await;
            if let Some(pool) = pool {
                let _ = queue::mark_failed(pool, job_id, &e.to_string()).await;
                let _ = queue::requeue_if_retryable(pool, job_id, attempts, 3).await;
            }
        }
        Err(_) => {
            crate::metrics::record_review_timeout();
            tracing::error!(job_id, "queued review timed out");
            release_claim_best_effort(repo, pr_number, head_sha).await;
            if let Some(pool) = pool {
                let _ = queue::mark_failed(pool, job_id, "timeout").await;
                let _ = queue::requeue_if_retryable(pool, job_id, attempts, 3).await;
            }
        }
    }

    drop(_guard);
    prune_pr_lock(repo, pr_number).await;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_webhook_review_inline(
    cfg: BotConfig,
    repo_full_name: String,
    pr_number: i64,
    pr: Option<serde_json::Value>,
    inst_id: Option<i64>,
    action: String,
    head_sha: String,
    timeout_secs: u64,
) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let lock = pr_lock(&repo_full_name, pr_number).await;
    let _guard = lock.lock().await;
    let started = std::time::Instant::now();
    let opts = ReviewOptions {
        auto_describe: matches!(
            action.as_str(),
            "opened" | "reopened" | "ready_for_review"
        ),
        auto_review_diff: true,
    };
    let repo_for_claim = repo_full_name.clone();
    let sha_for_claim = head_sha.clone();
    match timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&cfg, inst_id).await?;
        let wrapped = WebhookPayload {
            action: String::new(),
            pull_request: pr,
            repo: None,
            installation: None,
            comment: None,
            issue: None,
            repositories: None,
            repositories_added: None,
        };
        review_pr_with_options(&token, &repo_full_name, &wrapped, opts).await
    })
    .await
    {
        Ok(Ok(())) => {
            crate::metrics::record_review_ok(started);
            tracing::info!("review completed");
        }
        Ok(Err(e)) => {
            crate::metrics::record_review_failed();
            tracing::error!(error = %e, "review failed");
            release_claim_best_effort(&repo_for_claim, pr_number, &sha_for_claim).await;
        }
        Err(_) => {
            crate::metrics::record_review_timeout();
            tracing::error!("review timed out");
            release_claim_best_effort(&repo_for_claim, pr_number, &sha_for_claim).await;
        }
    }
    drop(_guard);
    prune_pr_lock(&repo_for_claim, pr_number).await;
}

pub(crate) async fn release_claim_best_effort(repo: &str, pr_number: i64, head_sha: &str) {
    if head_sha.is_empty() {
        return;
    }
    let Some(pool) = bot_db_pool() else {
        return;
    };
    let state = crate::state::ReviewState::from_pool(pool);
    if let Err(e) = state.release_sha_claim(repo, pr_number, head_sha).await {
        tracing::warn!(error = %e, "failed to release SHA claim");
    }
}
