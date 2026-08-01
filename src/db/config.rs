use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, db_fetch_optional, DbPool};

pub async fn get_config(pool: &DbPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let result: Option<AppConfig> =
        db_fetch_optional!(pool, AppConfig, "SELECT * FROM app_config WHERE key = ?", key)?;
    Ok(result.map(|c| c.value))
}

pub async fn set_config(pool: &DbPool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    db_execute!(
        pool,
        "INSERT INTO app_config (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
        key,
        value
    )?;
    Ok(())
}

pub async fn get_all_config(pool: &DbPool) -> Result<Vec<AppConfig>, sqlx::Error> {
    db_fetch_all!(pool, AppConfig, "SELECT * FROM app_config ORDER BY key")
}
