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

pub async fn create_audit_entry(
    pool: &DbPool,
    entry: &AuditEntryCreate,
) -> Result<AuditEntry, sqlx::Error> {
    sqlx::query_as::<_, AuditEntry>(
        "INSERT INTO audit_log (event_type, actor, target_type, target_id, metadata_json)
         VALUES (?, ?, ?, ?, ?)
         RETURNING *",
    )
    .bind(&entry.event_type)
    .bind(&entry.actor)
    .bind(&entry.target_type)
    .bind(entry.target_id)
    .bind(&entry.metadata_json)
    .fetch_one(&pool.0)
    .await
}
