pub mod audit;
pub mod config;
pub mod migrations;
pub mod models;
pub mod repos;
pub mod reviews;
pub mod users;

pub use models::*;

#[derive(Clone)]
pub struct DbPool(pub sqlx::Pool<sqlx::Any>);

impl std::ops::Deref for DbPool {
    type Target = sqlx::Pool<sqlx::Any>;

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
            return format!("{}{}{}", prefix, encoded, after_at);
        }
    }
    raw.to_string()
}

/// Detect whether a database URL points to PostgreSQL.
pub fn is_postgres_url(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

/// Create a connection pool from a database URL, run migrations, and return a `DbPool`.
/// Supports both SQLite (`sqlite://...`) and PostgreSQL (`postgres://...`).
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let url = if is_postgres_url(database_url) {
        normalize_database_url(database_url)
    } else {
        database_url.to_string()
    };

    let pool = sqlx::AnyPool::connect(&url).await?;
    let is_pg = is_postgres_url(database_url);
    migrations::run_migrations(&pool, is_pg).await?;
    Ok(DbPool(pool))
}
