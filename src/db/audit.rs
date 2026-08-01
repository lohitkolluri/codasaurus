use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, DbPool};

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
