pub mod audit;
pub mod config;
pub mod migrations;
pub mod models;
pub mod repos;
pub mod reviews;
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

/// Create a SQLite pool from a database URL, run migrations, and return a `DbPool`.
///
/// Default URL: `sqlite://codasaurus.db?mode=rwc` (create if not exists).
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let pool = sqlx::SqlitePool::connect(database_url).await?;
    migrations::run_migrations(&pool).await?;
    Ok(DbPool(pool))
}

/// Run embedded migrations on the given pool.
pub async fn run_migrations(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<(), sqlx::Error> {
    migrations::run_migrations(pool).await
}
