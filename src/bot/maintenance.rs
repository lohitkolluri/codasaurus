//! Periodic DB maintenance (sessions, webhooks, finished jobs).

use crate::db::{db_execute, DbPool};

/// Opportunistic cleanup — safe to call often; each statement is cheap with indexes.
pub async fn run_periodic_cleanup(pool: &DbPool) {
    // Expired sessions (also cleaned on auth lookup).
    if let Err(e) = db_execute!(
        pool,
        "DELETE FROM sessions WHERE expires_at < datetime('now')"
    ) {
        tracing::debug!(error = %e, "session cleanup skipped");
    }

    // Webhook delivery dedup retention (14 days).
    if let Err(e) = db_execute!(
        pool,
        "DELETE FROM webhook_deliveries WHERE received_at < datetime('now', '-14 days')"
    ) {
        tracing::debug!(error = %e, "webhook cleanup skipped");
    }

    // Finished queue rows older than 30 days.
    if let Err(e) = db_execute!(
        pool,
        "DELETE FROM review_jobs
         WHERE status IN ('done', 'failed')
           AND updated_at < datetime('now', '-30 days')"
    ) {
        tracing::debug!(error = %e, "review_jobs cleanup skipped");
    }
}
