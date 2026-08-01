//! Review comment IDs and commit SHA tracking.
//! Uses the shared app DbPool when available; falls back to a dedicated SQLite file.

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

/// Stale `in_progress` claims older than this are reclaimable (crash / timeout recovery).
const STALE_CLAIM_SECS: i64 = 600;

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

    /// Open the dedicated review_state.db (non-serve / test contexts).
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

    fn comment_key(repo: &str, pr_number: i64, kind: &str) -> String {
        format!("{repo}/{pr_number}:{kind}")
    }

    /// Legacy key without kind (pre–comment-slot split).
    fn legacy_comment_key(repo: &str, pr_number: i64) -> String {
        format!("{repo}/{pr_number}")
    }

    pub async fn get_comment_id_async(
        &self,
        repo: &str,
        pr_number: i64,
        kind: &str,
    ) -> Result<Option<i64>> {
        let key = Self::comment_key(repo, pr_number, kind);
        let result: Option<(i64,)> =
            sqlx::query_as("SELECT comment_id FROM review_comments WHERE repo_pr = ?")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await?;
        if result.is_some() {
            return Ok(result.map(|r| r.0));
        }
        // Walkthrough falls back to legacy single-slot key once.
        if kind == "walkthrough" {
            let legacy = Self::legacy_comment_key(repo, pr_number);
            let result: Option<(i64,)> =
                sqlx::query_as("SELECT comment_id FROM review_comments WHERE repo_pr = ?")
                    .bind(&legacy)
                    .fetch_optional(&self.pool)
                    .await?;
            return Ok(result.map(|r| r.0));
        }
        Ok(None)
    }

    pub async fn set_comment_id_async(
        &self,
        repo: &str,
        pr_number: i64,
        kind: &str,
        comment_id: i64,
    ) -> Result<()> {
        let key = Self::comment_key(repo, pr_number, kind);
        sqlx::query(
            "INSERT OR REPLACE INTO review_comments (repo_pr, comment_id) VALUES (?, ?)",
        )
        .bind(&key)
        .bind(comment_id)
        .execute(&self.pool)
        .await?;
        Ok(())
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
    /// completed or is actively in progress (and not stale).
    pub async fn try_claim_sha(&self, repo: &str, pr_number: i64, sha: &str) -> Result<bool> {
        let key = format!("{repo}/{pr_number}");
        if let Some((existing, status, created_at)) = sqlx::query_as::<_, (String, String, String)>(
            "SELECT head_sha, COALESCE(status, 'completed'), COALESCE(created_at, datetime('now'))
             FROM reviewed_commits WHERE repo_pr = ?",
        )
        .bind(&key)
        .fetch_optional(&self.pool)
        .await?
        {
            if existing == sha {
                if status == "completed" {
                    return Ok(false);
                }
                // Same SHA stuck in_progress — reclaim if stale.
                if status == "in_progress" && !is_claim_stale(&created_at) {
                    return Ok(false);
                }
            }
        }

        let result = sqlx::query(
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status, created_at)
             VALUES (?, ?, 'in_progress', datetime('now'))
             ON CONFLICT(repo_pr) DO UPDATE SET
               head_sha = excluded.head_sha,
               status = 'in_progress',
               created_at = datetime('now')
             WHERE reviewed_commits.head_sha != excluded.head_sha
                OR reviewed_commits.status != 'in_progress'
                OR reviewed_commits.created_at < datetime('now', ?)",
        )
        .bind(&key)
        .bind(sha)
        .bind(format!("-{STALE_CLAIM_SECS} seconds"))
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Mark claim completed (success path).
    pub async fn set_reviewed_sha_async(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        sqlx::query(
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status, created_at)
             VALUES (?, ?, 'completed', datetime('now'))
             ON CONFLICT(repo_pr) DO UPDATE SET
               head_sha = excluded.head_sha,
               status = 'completed',
               created_at = datetime('now')",
        )
        .bind(&key)
        .bind(sha)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Clear an in_progress claim so a timed-out / failed review can be retried.
    pub async fn release_sha_claim(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        sqlx::query(
            "DELETE FROM reviewed_commits
             WHERE repo_pr = ? AND head_sha = ? AND status = 'in_progress'",
        )
        .bind(&key)
        .bind(sha)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn is_claim_stale(created_at: &str) -> bool {
    // SQLite datetime('now') is UTC `YYYY-MM-DD HH:MM:SS`. Parse loosely.
    let Ok(naive) = chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S") else {
        return true; // unparseable → allow reclaim
    };
    let created = naive.and_utc();
    let age = chrono::Utc::now().signed_duration_since(created);
    age.num_seconds() >= STALE_CLAIM_SECS
}
