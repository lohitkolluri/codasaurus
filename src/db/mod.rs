pub mod audit;
pub mod config;
pub mod dialect;
pub mod events;
pub mod migrations;
pub mod models;
pub mod repos;
pub mod reviews;
pub mod sessions;
pub mod users;

pub use models::*;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

/// PostgreSQL connection pool (sole durable store).
#[derive(Clone)]
pub struct DbPool(PgPool);

impl DbPool {
    pub fn as_pg(&self) -> &PgPool {
        &self.0
    }

    /// Adapt `?` placeholders and datetime helpers to Postgres SQL.
    pub fn prepare_sql(&self, sql: &str) -> String {
        dialect::prepare(sql)
    }

    /// Lightweight connectivity probe.
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(self.as_pg())
            .await
            .map(|_| ())
    }
}

/// Normalize a database URL by percent-encoding special characters in the
/// password portion (like `@`, `:`, `#`, `%`, `?`). Users often paste raw
/// passwords into connection strings, which breaks URL parsing.
pub fn normalize_database_url(raw: &str) -> String {
    if !raw.starts_with("postgres://") && !raw.starts_with("postgresql://") {
        return raw.to_string();
    }
    if url::Url::parse(raw).is_ok() {
        return raw.to_string();
    }
    if let Some(at_pos) = raw.rfind('@') {
        let before_at = &raw[..at_pos];
        let after_at = &raw[at_pos..];
        if let Some(colon_pos) = before_at.rfind(':') {
            let prefix = &before_at[..=colon_pos];
            let password = &before_at[colon_pos + 1..];
            let encoded: String = password
                .chars()
                .map(|c| match c {
                    '@' | ':' | '%' | '#' | '?' | ' ' | '/' | '\\' => {
                        format!("%{:02X}", c as u8)
                    }
                    _ => c.to_string(),
                })
                .collect();
            return format!("{prefix}{encoded}{after_at}");
        }
    }
    raw.to_string()
}

/// Create a Postgres pool from `DATABASE_URL` and run migrations.
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let normalized = normalize_database_url(database_url);
    if !normalized.starts_with("postgres://") && !normalized.starts_with("postgresql://") {
        return Err(sqlx::Error::Configuration(
            format!(
                "DATABASE_URL must be a postgres:// or postgresql:// URL (got {})",
                if normalized.is_empty() {
                    "empty".into()
                } else {
                    normalized
                        .split(':')
                        .next()
                        .unwrap_or("unknown")
                        .to_string()
                        + "://"
                }
            )
            .into(),
        ));
    }

    // Size for API + review workers without oversubscribing Postgres.
    // Formula guidance: ~(cores×2)+spindle; we cap at 16 for single-node self-host.
    let max_connections = std::env::var("CODASAURUS_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16u32)
        .clamp(2, 64);

    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .test_before_acquire(false)
        .connect(&normalized)
        .await?;
    migrations::run_migrations(&pool).await?;
    Ok(DbPool(pool))
}

mod macros;

pub(crate) use macros::{
    db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, db_scalar, db_scalar_optional,
};
