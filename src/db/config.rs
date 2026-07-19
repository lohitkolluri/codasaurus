use crate::db::models::*;
use crate::db::DbPool;

pub async fn get_config(pool: &DbPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let result: Option<AppConfig> =
        sqlx::query_as::<_, AppConfig>("SELECT * FROM app_config WHERE key = ?")
            .bind(key)
            .fetch_optional(&pool.0)
            .await?;
    Ok(result.map(|c| c.value))
}

pub async fn set_config(pool: &DbPool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app_config (key, value, updated_at) VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(key)
    .bind(value)
    .execute(&pool.0)
    .await?;
    Ok(())
}

pub async fn get_all_config(pool: &DbPool) -> Result<Vec<AppConfig>, sqlx::Error> {
    sqlx::query_as::<_, AppConfig>("SELECT * FROM app_config ORDER BY key")
        .fetch_all(&pool.0)
        .await
}
