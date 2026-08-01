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
        sqlx::query_scalar::<_, i32>("SELECT 1")
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

/// Ensure remote cloud Postgres URLs request TLS (`sslmode=require`).
/// Local / Docker Compose hosts are left alone.
pub fn ensure_sslmode(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let local = host.is_empty()
        || host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host == "postgres" // compose service name
        || host.ends_with(".local");
    if local {
        return url.to_string();
    }
    let has_ssl = parsed
        .query_pairs()
        .any(|(k, _)| k.eq_ignore_ascii_case("sslmode") || k.eq_ignore_ascii_case("ssl"));
    if has_ssl {
        return url.to_string();
    }
    if url.contains('?') {
        format!("{url}&sslmode=require")
    } else {
        format!("{url}?sslmode=require")
    }
}

fn free_tier_hints(database_url: &str) -> bool {
    if std::env::var_os("CODASAURUS_FREE_TIER").is_some_and(|v| v != "0" && v != "false") {
        return true;
    }
    if std::env::var_os("RENDER").is_some() || std::env::var_os("RENDER_SERVICE_ID").is_some() {
        return true;
    }
    let lower = database_url.to_ascii_lowercase();
    [
        "neon.tech",
        "neon.cloud",
        "supabase.co",
        "supabase.com",
        "aivencloud.com",
        "aiven.io",
        ".render.com",
        "amazonaws.com", // often tiny free/aurora trials
    ]
    .iter()
    .any(|h| lower.contains(h))
}

fn default_max_connections(database_url: &str) -> u32 {
    if free_tier_hints(database_url) {
        3
    } else {
        16
    }
}

fn default_acquire_timeout_secs(database_url: &str) -> u64 {
    // Neon / free DBs may cold-start; give them time to wake.
    if free_tier_hints(database_url) {
        60
    } else {
        30
    }
}

/// Create a Postgres pool from `DATABASE_URL` and run migrations.
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let normalized = ensure_sslmode(&normalize_database_url(database_url));
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

    let max_connections = std::env::var("CODASAURUS_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| default_max_connections(&normalized))
        .clamp(2, 64);

    let acquire_secs = std::env::var("CODASAURUS_DB_ACQUIRE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| default_acquire_timeout_secs(&normalized))
        .clamp(5, 120);

    tracing::info!(
        max_connections,
        acquire_secs,
        free_tier = free_tier_hints(&normalized),
        "connecting to PostgreSQL"
    );

    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(acquire_secs))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .test_before_acquire(false)
        .connect(&normalized)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                "PostgreSQL connect failed — check DATABASE_URL, SSL (sslmode=require), and that the DB is reachable (Render+Neon is the recommended free stack; see docs/run-for-free.md)"
            );
            e
        })?;
    migrations::run_migrations(&pool).await?;
    Ok(DbPool(pool))
}

mod macros;

pub(crate) use macros::{
    db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, db_scalar, db_scalar_optional,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_sslmode_for_remote_hosts() {
        let u = ensure_sslmode("postgres://u:p@db.example.com:5432/app");
        assert!(u.contains("sslmode=require"));
    }

    #[test]
    fn skips_sslmode_for_localhost() {
        let u = ensure_sslmode("postgres://u:p@127.0.0.1:5432/app");
        assert!(!u.contains("sslmode"));
    }

    #[test]
    fn preserves_existing_sslmode() {
        let u = ensure_sslmode("postgres://u:p@db.example.com/app?sslmode=verify-full");
        assert!(u.contains("sslmode=verify-full"));
        assert!(!u.contains("sslmode=require"));
    }

    #[test]
    fn free_tier_detects_neon() {
        assert!(free_tier_hints("postgres://u:p@ep-x.neon.tech/neondb"));
    }

    #[test]
    fn free_tier_skips_local() {
        assert!(!free_tier_hints("postgres://u:p@127.0.0.1:5432/app"));
    }
}
