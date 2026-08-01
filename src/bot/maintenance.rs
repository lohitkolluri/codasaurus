//! Periodic DB maintenance (sessions, webhooks, finished jobs).

use crate::db::{db_execute, DbPool};

/// Opportunistic cleanup — safe to call often; each statement is cheap with indexes.
pub async fn run_periodic_cleanup(pool: &DbPool) {
    if let Err(e) = db_execute!(pool, "DELETE FROM sessions WHERE expires_at < NOW()") {
        tracing::debug!(error = %e, "session cleanup skipped");
    }

    if let Err(e) = db_execute!(
        pool,
        "DELETE FROM webhook_deliveries WHERE received_at < NOW() - INTERVAL '14 days'"
    ) {
        tracing::debug!(error = %e, "webhook cleanup skipped");
    }

    if let Err(e) = db_execute!(
        pool,
        "DELETE FROM review_jobs
         WHERE status IN ('done', 'failed')
           AND updated_at < NOW() - INTERVAL '30 days'"
    ) {
        tracing::debug!(error = %e, "review_jobs cleanup skipped");
    }

    crate::db::events::prune_older_than_days(pool, 30).await;
}
