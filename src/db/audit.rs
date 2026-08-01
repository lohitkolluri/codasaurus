use crate::db::models::*;
use crate::db::DbPool;

pub async fn list_audit_entries(
    pool: &DbPool,
    event_type: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<AuditEntry>, sqlx::Error> {
    match event_type {
        Some(et) => {
            let entries = sqlx::query_as::<_, AuditEntry>(
                "SELECT * FROM audit_log WHERE event_type = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(et)
            .bind(limit)
            .bind(offset)
            .fetch_all(&pool.0)
            .await?;
            Ok(entries)
        }
        None => {
            sqlx::query_as::<_, AuditEntry>(
                "SELECT * FROM audit_log ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&pool.0)
            .await
        }
    }
}

pub async fn log_event(
    pool: &DbPool,
    event_type: &str,
    actor: Option<&str>,
    target_type: Option<&str>,
    target_id: Option<i64>,
) {
    match sqlx::query(
        "INSERT INTO audit_log (event_type, actor, target_type, target_id) VALUES (?, ?, ?, ?)",
    )
    .bind(event_type)
    .bind(actor)
    .bind(target_type)
    .bind(target_id)
    .execute(&pool.0)
    .await
    {
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, event_type, "failed to write audit log"),
    }
}
