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
pub struct DbPool(pub sqlx::Pool<sqlx::Postgres>);

impl std::ops::Deref for DbPool {
    type Target = sqlx::Pool<sqlx::Postgres>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Normalize a database URL by percent-encoding special characters in the
/// password portion (like `@`, `:`, `#`, `%`, `?`). Users often paste raw
/// passwords into connection strings, which breaks URL parsing.
pub fn normalize_database_url(raw: &str) -> String {
    if raw.starts_with("postgres://") || raw.starts_with("postgresql://") {
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
    }
    raw.to_string()
}

/// Create a PostgreSQL pool, run migrations, and return a `DbPool`.
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let url = if database_url.starts_with("sqlite://") {
        // Legacy SQLite URL — needs PostgreSQL migration
        eprintln!("  Legacy SQLite URL detected. Switch to postgres:// URL.");
        return Err(sqlx::Error::Protocol(
            "SQLite is no longer supported. Use a postgres:// DATABASE_URL.".into(),
        ));
    } else {
        normalize_database_url(database_url)
    };
    let pool = sqlx::PgPool::connect(&url).await?;
    migrations::run_migrations(&pool).await?;
    Ok(DbPool(pool))
}
