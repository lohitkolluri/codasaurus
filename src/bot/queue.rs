//! Durable SQLite-backed review job queue.

use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct ReviewJob {
    pub id: i64,
    pub repo: String,
    pub pr_number: i64,
    pub head_sha: String,
    pub installation_id: Option<i64>,
    pub action: String,
    pub attempts: i64,
}

/// Enqueue (or refresh) a review job for this SHA. Idempotent per (repo, pr, sha).
pub async fn enqueue(
    pool: &SqlitePool,
    repo: &str,
    pr_number: i64,
    head_sha: &str,
    installation_id: Option<i64>,
    action: &str,
) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO review_jobs (repo, pr_number, head_sha, installation_id, action, status, attempts, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'pending', 0, datetime('now'), datetime('now'))
         ON CONFLICT(repo, pr_number, head_sha) DO UPDATE SET
           status = CASE
             WHEN review_jobs.status IN ('done', 'failed') THEN 'pending'
             ELSE review_jobs.status
           END,
           installation_id = excluded.installation_id,
           action = excluded.action,
           updated_at = datetime('now')
         RETURNING id",
    )
    .bind(repo)
    .bind(pr_number)
    .bind(head_sha)
    .bind(installation_id)
    .bind(action)
    .fetch_one(pool)
    .await?;
    crate::metrics::record_queue_enqueued();
    Ok(row.0)
}

/// Atomically claim the oldest pending (or stale running) job.
pub async fn claim_next(pool: &SqlitePool, stale_secs: i64) -> Result<Option<ReviewJob>> {
    let stale = format!("-{stale_secs} seconds");
    // Prefer true pending; also reclaim stale running rows.
    let row = sqlx::query_as::<_, (i64, String, i64, String, Option<i64>, String, i64)>(
        "UPDATE review_jobs SET
           status = 'running',
           attempts = attempts + 1,
           updated_at = datetime('now')
         WHERE id = (
           SELECT id FROM review_jobs
           WHERE status = 'pending'
              OR (status = 'running' AND updated_at < datetime('now', ?))
           ORDER BY created_at ASC
           LIMIT 1
         )
         RETURNING id, repo, pr_number, head_sha, installation_id, action, attempts",
    )
    .bind(&stale)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, repo, pr_number, head_sha, installation_id, action, attempts)| ReviewJob {
            id,
            repo,
            pr_number,
            head_sha,
            installation_id,
            action,
            attempts,
        },
    ))
}

pub async fn mark_done(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE review_jobs SET status = 'done', updated_at = datetime('now'), last_error = NULL WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    crate::metrics::record_queue_completed();
    Ok(())
}

pub async fn mark_failed(pool: &SqlitePool, id: i64, err: &str) -> Result<()> {
    let msg: String = err.chars().take(500).collect();
    sqlx::query(
        "UPDATE review_jobs SET status = 'failed', updated_at = datetime('now'), last_error = ? WHERE id = ?",
    )
    .bind(&msg)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Re-queue a failed/timed-out job if under max attempts.
pub async fn requeue_if_retryable(pool: &SqlitePool, id: i64, attempts: i64, max_attempts: i64) -> Result<()> {
    if attempts >= max_attempts {
        return Ok(());
    }
    sqlx::query(
        "UPDATE review_jobs SET status = 'pending', updated_at = datetime('now') WHERE id = ? AND status = 'failed'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
