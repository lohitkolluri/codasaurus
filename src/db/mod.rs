pub mod audit;
pub mod config;
pub mod migrations;
pub mod models;
pub mod repos;
pub mod reviews;
pub mod sessions;
pub mod users;

pub use models::*;

#[derive(Clone)]
pub struct DbPool(pub sqlx::Pool<sqlx::Sqlite>);

impl std::ops::Deref for DbPool {
    type Target = sqlx::Pool<sqlx::Sqlite>;

    fn deref(&self) -> &Self::Target {
        &self.0
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

/// Create a SQLite pool from a database URL, run migrations, and return a `DbPool`.
///
/// Runtime storage is SQLite-only in this release. Postgres URLs are rejected with a
/// clear error (wizard connection tests may still validate Postgres separately).
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let normalized = normalize_database_url(database_url);
    if normalized.starts_with("postgres://") || normalized.starts_with("postgresql://") {
        return Err(sqlx::Error::Configuration(
            "PostgreSQL runtime is not enabled yet — set DATABASE_URL to sqlite://… (e.g. sqlite:///data/codasaurus.db?mode=rwc)".into(),
        ));
    }
    let pool = sqlx::SqlitePool::connect(&normalized).await?;
    migrations::run_migrations(&pool).await?;
    Ok(DbPool(pool))
}
