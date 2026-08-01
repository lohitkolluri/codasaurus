use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, DbPool};

/// Default audit log retention (days). Override with `CODASAURUS_AUDIT_RETENTION_DAYS`.
pub const DEFAULT_RETENTION_DAYS: i64 = 90;

pub fn retention_days() -> i64 {
    std::env::var("CODASAURUS_AUDIT_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS)
        .clamp(7, 730)
}

pub async fn list_audit_entries(
    pool: &DbPool,
    event_type: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<AuditEntry>, sqlx::Error> {
    match event_type {
        Some(et) => db_fetch_all!(
            pool,
            AuditEntry,
            "SELECT * FROM audit_log WHERE event_type = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            et,
            limit,
            offset
        ),
        None => db_fetch_all!(
            pool,
            AuditEntry,
            "SELECT * FROM audit_log ORDER BY created_at DESC LIMIT ? OFFSET ?",
            limit,
            offset
        ),
    }
}

pub async fn log_event(
    pool: &DbPool,
    event_type: &str,
    actor: Option<&str>,
    target_type: Option<&str>,
    target_id: Option<i64>,
) {
    match db_execute!(
        pool,
        "INSERT INTO audit_log (event_type, actor, target_type, target_id) VALUES (?, ?, ?, ?)",
        event_type,
        actor,
        target_type,
        target_id
    ) {
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, event_type, "failed to write audit log"),
    }
}

/// Drop audit rows older than the configured retention window.
pub async fn prune_older_than_days(pool: &DbPool, days: i64) {
    let days = days.clamp(7, 730);
    if let Err(e) = sqlx::query(
        "DELETE FROM audit_log WHERE created_at < NOW() - ($1::bigint * INTERVAL '1 day')",
    )
    .bind(days)
    .execute(pool.as_pg())
    .await
    {
        tracing::debug!(error = %e, "audit_log prune skipped");
    }
}
