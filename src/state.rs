//! Review comment IDs and commit SHA tracking.
//! Uses the shared app DbPool when available; falls back to a dedicated SQLite file for CLI.

use anyhow::Result;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::LazyLock;
use tokio::runtime::{Handle, Runtime};

static FALLBACK_RT: LazyLock<Runtime> =
    LazyLock::new(|| Runtime::new().expect("failed to create fallback tokio runtime"));

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    match Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => FALLBACK_RT.block_on(fut),
    }
}

pub struct ReviewState {
    pool: SqlitePool,
}

impl ReviewState {
    /// Use the shared bot/app database pool (preferred for serve/bot paths).
    pub fn from_pool(pool: &crate::db::DbPool) -> Self {
        Self {
            pool: pool.0.clone(),
        }
    }

    /// Open the dedicated review_state.db (CLI / non-serve contexts).
    pub fn open() -> Result<Self> {
        Self::open_at(Self::db_path()?)
    }

    fn open_at(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = block_on(SqlitePool::connect(&url))?;
        let store = Self { pool };
        store.initialize()?;
        Ok(store)
    }

    fn db_path() -> Result<PathBuf> {
        Ok(crate::storage::data_dir().join("review_state.db"))
    }

    fn initialize(&self) -> Result<()> {
        block_on(async {
            sqlx::query("PRAGMA journal_mode = WAL")
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
                    status TEXT NOT NULL DEFAULT 'completed',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 )",
            )
            .execute(&self.pool)
            .await?;
            Ok::<_, anyhow::Error>(())
        })
    }

    pub fn get_comment_id(&self, repo: &str, pr_number: i64) -> Result<Option<i64>> {
        block_on(self.get_comment_id_async(repo, pr_number))
    }

    pub async fn get_comment_id_async(&self, repo: &str, pr_number: i64) -> Result<Option<i64>> {
        let key = format!("{repo}/{pr_number}");
        let result: Option<(i64,)> =
            sqlx::query_as("SELECT comment_id FROM review_comments WHERE repo_pr = ?")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(result.map(|r| r.0))
    }

    pub fn get_reviewed_sha(&self, repo: &str, pr_number: i64) -> Result<Option<String>> {
        block_on(self.get_reviewed_sha_async(repo, pr_number))
    }

    pub async fn get_reviewed_sha_async(&self, repo: &str, pr_number: i64) -> Result<Option<String>> {
        let key = format!("{repo}/{pr_number}");
        let result: Option<(String,)> =
            sqlx::query_as("SELECT head_sha FROM reviewed_commits WHERE repo_pr = ?")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(result.map(|r| r.0))
    }

    /// Atomically claim a SHA for review. Returns false if this SHA was already
    /// claimed/completed (another worker is reviewing or finished).
    pub async fn try_claim_sha(&self, repo: &str, pr_number: i64, sha: &str) -> Result<bool> {
        let key = format!("{repo}/{pr_number}");
        // If same SHA already recorded, skip
        if let Some((existing, status)) = sqlx::query_as::<_, (String, String)>(
            "SELECT head_sha, COALESCE(status, 'completed') FROM reviewed_commits WHERE repo_pr = ?",
        )
        .bind(&key)
        .fetch_optional(&self.pool)
        .await?
        {
            if existing == sha {
                return Ok(false);
            }
            let _ = status;
        }

        let result = sqlx::query(
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status) VALUES (?, ?, 'in_progress')
             ON CONFLICT(repo_pr) DO UPDATE SET head_sha = excluded.head_sha, status = 'in_progress'
             WHERE reviewed_commits.head_sha != excluded.head_sha
                OR reviewed_commits.status != 'in_progress'",
        )
        .bind(&key)
        .bind(sha)
        .execute(&self.pool)
        .await?;

        // rows_affected == 0 means conflict and WHERE didn't match (same sha in progress)
        Ok(result.rows_affected() > 0)
    }

    pub fn set_reviewed_sha(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        block_on(self.set_reviewed_sha_async(repo, pr_number, sha))
    }

    pub async fn set_reviewed_sha_async(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        sqlx::query(
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status) VALUES (?, ?, 'completed')
             ON CONFLICT(repo_pr) DO UPDATE SET head_sha = excluded.head_sha, status = 'completed'",
        )
        .bind(&key)
        .bind(sha)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub fn set_comment_id(&self, repo: &str, pr_number: i64, comment_id: i64) -> Result<()> {
        block_on(self.set_comment_id_async(repo, pr_number, comment_id))
    }

    pub async fn set_comment_id_async(
        &self,
        repo: &str,
        pr_number: i64,
        comment_id: i64,
    ) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        sqlx::query(
            "INSERT OR REPLACE INTO review_comments (repo_pr, comment_id) VALUES (?, ?)",
        )
        .bind(&key)
        .bind(comment_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
