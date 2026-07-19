//! Lightweight state store for tracking review comment IDs and commit SHAs.
//! Keyed by `{repo_owner}/{repo_name}/{pr_number}`.
//! Uses SQLite with WAL mode for persistence across bot restarts.

use anyhow::Result;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tokio::runtime::Runtime;

pub struct ReviewState {
    pool: SqlitePool,
    rt: Runtime,
}

impl ReviewState {
    pub fn open() -> Result<Self> {
        Self::open_at(Self::db_path()?)
    }

    fn open_at(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let rt = Runtime::new()?;
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = rt.block_on(SqlitePool::connect(&url))?;
        let store = Self { pool, rt };
        store.initialize()?;
        Ok(store)
    }

    fn db_path() -> Result<PathBuf> {
        Ok(crate::storage::data_dir().join("review_state.db"))
    }

    fn initialize(&self) -> Result<()> {
        self.rt.block_on(async {
            sqlx::query("PRAGMA journal_mode = WAL")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA synchronous = NORMAL")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA cache_size = -8000")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA busy_timeout = 5000")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&self.pool)
                .await?;
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS review_comments (
                    repo_pr TEXT PRIMARY KEY,
                    comment_id INTEGER NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 )",
            )
            .execute(&self.pool)
            .await?;
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS reviewed_commits (
                    repo_pr TEXT PRIMARY KEY,
                    head_sha TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 )",
            )
            .execute(&self.pool)
            .await
        })?;
        Ok(())
    }

    /// Get the stored comment ID for a repo+PR combination.
    pub fn get_comment_id(&self, repo: &str, pr_number: i64) -> Result<Option<i64>> {
        let key = format!("{}/{}", repo, pr_number);
        self.rt.block_on(async {
            let result: Option<(i64,)> =
                sqlx::query_as("SELECT comment_id FROM review_comments WHERE repo_pr = ?")
                    .bind(&key)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok(result.map(|r| r.0))
        })
    }

    /// Get the last reviewed commit SHA for a repo+PR combination.
    pub fn get_reviewed_sha(&self, repo: &str, pr_number: i64) -> Result<Option<String>> {
        let key = format!("{}/{}", repo, pr_number);
        self.rt.block_on(async {
            let result: Option<(String,)> =
                sqlx::query_as("SELECT head_sha FROM reviewed_commits WHERE repo_pr = ?")
                    .bind(&key)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok(result.map(|r| r.0))
        })
    }

    /// Store or update the last reviewed commit SHA.
    pub fn set_reviewed_sha(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let key = format!("{}/{}", repo, pr_number);
        self.rt.block_on(async {
            sqlx::query("INSERT OR REPLACE INTO reviewed_commits (repo_pr, head_sha) VALUES (?, ?)")
                .bind(&key)
                .bind(sha)
                .execute(&self.pool)
                .await
        })?;
        Ok(())
    }

    /// Store or update the comment ID for a repo+PR combination.
    pub fn set_comment_id(&self, repo: &str, pr_number: i64, comment_id: i64) -> Result<()> {
        let key = format!("{}/{}", repo, pr_number);
        self.rt.block_on(async {
            sqlx::query(
                "INSERT OR REPLACE INTO review_comments (repo_pr, comment_id) VALUES (?, ?)",
            )
            .bind(&key)
            .bind(comment_id)
            .execute(&self.pool)
            .await
        })?;
        Ok(())
    }
}
