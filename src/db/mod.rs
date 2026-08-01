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
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{PgPool, SqlitePool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

/// Dual-backend pool: SQLite (default) or Postgres (production HA).
#[derive(Clone)]
pub enum DbPool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl DbPool {
    pub fn backend(&self) -> Backend {
        match self {
            Self::Sqlite(_) => Backend::Sqlite,
            Self::Postgres(_) => Backend::Postgres,
        }
    }

    pub fn is_postgres(&self) -> bool {
        matches!(self, Self::Postgres(_))
    }

    pub fn prepare_sql(&self, sql: &str) -> String {
        dialect::prepare(sql, self.backend())
    }

    /// Lightweight connectivity probe.
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        match self {
            Self::Sqlite(p) => {
                sqlx::query_scalar::<_, i64>("SELECT 1")
                    .fetch_one(p)
                    .await
                    .map(|_| ())
            }
            Self::Postgres(p) => {
                sqlx::query_scalar::<_, i64>("SELECT 1")
                    .fetch_one(p)
                    .await
                    .map(|_| ())
            }
        }
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

/// Create a pool from `DATABASE_URL` (sqlite:// or postgres://), run migrations.
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let normalized = normalize_database_url(database_url);
    if normalized.starts_with("postgres://") || normalized.starts_with("postgresql://") {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(&normalized)
            .await?;
        migrations::run_migrations_postgres(&pool).await?;
        Ok(DbPool::Postgres(pool))
    } else {
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect(&normalized)
            .await?;
        migrations::run_migrations_sqlite(&pool).await?;
        Ok(DbPool::Sqlite(pool))
    }
}

mod macros;

pub(crate) use macros::{
    db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, db_scalar, db_scalar_optional,
};
