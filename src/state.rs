//! Review comment IDs and commit SHA tracking (multi-replica safe leases).

use anyhow::Result;
use crate::db::{db_execute, db_fetch_optional, db_scalar_optional, DbPool};
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};
use tokio::runtime::{Handle, Runtime};

static FALLBACK_RT: LazyLock<Runtime> =
    LazyLock::new(|| Runtime::new().expect("failed to create fallback tokio runtime"));

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    match Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => FALLBACK_RT.block_on(fut),
    }
}

const STALE_CLAIM_SECS: i64 = 600;

fn lease_owner_id() -> &'static str {
    static OWNER: OnceLock<String> = OnceLock::new();
    OWNER.get_or_init(|| {
        let host = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("CODASAURUS_INSTANCE_ID"))
            .unwrap_or_else(|_| "node".into());
        format!("{host}-{}", std::process::id())
    })
}

pub struct ReviewState {
    pool: DbPool,
}

impl ReviewState {
    pub fn from_pool(pool: &DbPool) -> Self {
        Self {
            pool: pool.clone(),
        }
    }

    pub fn open() -> Result<Self> {
        Self::open_at(Self::db_path()?)
    }

    fn open_at(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = block_on(crate::db::create_pool(&url))?;
        let store = Self { pool };
        // create_pool already migrates app schema; ensure lease tables exist via migrations.
        Ok(store)
    }

    fn db_path() -> Result<PathBuf> {
        Ok(crate::storage::data_dir().join("review_state.db"))
    }

    fn comment_key(repo: &str, pr_number: i64, kind: &str) -> String {
        format!("{repo}/{pr_number}:{kind}")
    }

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
        let result: Option<i64> = db_scalar_optional!(
            &self.pool,
            i64,
            "SELECT comment_id FROM review_comments WHERE repo_pr = ?",
            &key
        )?;
        if result.is_some() {
            return Ok(result);
        }
        if kind == "walkthrough" {
            let legacy = Self::legacy_comment_key(repo, pr_number);
            let result: Option<i64> = db_scalar_optional!(
                &self.pool,
                i64,
                "SELECT comment_id FROM review_comments WHERE repo_pr = ?",
                &legacy
            )?;
            return Ok(result);
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
        db_execute!(
            &self.pool,
            "INSERT INTO review_comments (repo_pr, comment_id) VALUES (?, ?)
             ON CONFLICT(repo_pr) DO UPDATE SET comment_id = excluded.comment_id",
            &key,
            comment_id
        )?;
        Ok(())
    }

    pub async fn get_reviewed_sha_async(&self, repo: &str, pr_number: i64) -> Result<Option<String>> {
        let key = format!("{repo}/{pr_number}");
        Ok(db_scalar_optional!(
            &self.pool,
            String,
            "SELECT head_sha FROM reviewed_commits WHERE repo_pr = ?",
            &key
        )?)
    }

    pub async fn try_claim_sha(&self, repo: &str, pr_number: i64, sha: &str) -> Result<bool> {
        let key = format!("{repo}/{pr_number}");
        let owner = lease_owner_id();
        if let Some(row) = db_fetch_optional!(
            &self.pool,
            (String, String, String, String),
            "SELECT head_sha,
                    COALESCE(status, 'completed'),
                    COALESCE(created_at, datetime('now')),
                    COALESCE(lease_owner, '')
             FROM reviewed_commits WHERE repo_pr = ?",
            &key
        )? {
            let (existing, status, created_at, lease_owner) = row;
            if existing == sha {
                if status == "completed" {
                    return Ok(false);
                }
                if status == "in_progress" && lease_owner != owner && !is_claim_stale(&created_at) {
                    return Ok(false);
                }
                if status == "in_progress" && lease_owner == owner && !is_claim_stale(&created_at) {
                    return Ok(false);
                }
            }
        }

        let stale = format!("-{STALE_CLAIM_SECS} seconds");
        let result = db_execute!(
            &self.pool,
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status, lease_owner, created_at)
             VALUES (?, ?, 'in_progress', ?, datetime('now'))
             ON CONFLICT(repo_pr) DO UPDATE SET
               head_sha = excluded.head_sha,
               status = 'in_progress',
               lease_owner = excluded.lease_owner,
               created_at = datetime('now')
             WHERE reviewed_commits.head_sha != excluded.head_sha
                OR reviewed_commits.status != 'in_progress'
                OR reviewed_commits.lease_owner = excluded.lease_owner
                OR reviewed_commits.created_at < datetime('now', ?)",
            &key,
            sha,
            owner,
            &stale
        )?;

        Ok(result > 0)
    }

    pub async fn set_reviewed_sha_async(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        let owner = lease_owner_id();
        db_execute!(
            &self.pool,
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status, lease_owner, created_at)
             VALUES (?, ?, 'completed', ?, datetime('now'))
             ON CONFLICT(repo_pr) DO UPDATE SET
               head_sha = excluded.head_sha,
               status = 'completed',
               lease_owner = excluded.lease_owner,
               created_at = datetime('now')",
            &key,
            sha,
            owner
        )?;
        Ok(())
    }

    pub async fn release_sha_claim(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        let owner = lease_owner_id();
        let stale = format!("-{STALE_CLAIM_SECS} seconds");
        db_execute!(
            &self.pool,
            "DELETE FROM reviewed_commits
             WHERE repo_pr = ? AND head_sha = ? AND status = 'in_progress'
               AND (lease_owner = ? OR lease_owner = '' OR created_at < datetime('now', ?))",
            &key,
            sha,
            owner,
            &stale
        )?;
        Ok(())
    }
}

fn is_claim_stale(created_at: &str) -> bool {
    let Ok(naive) = chrono::NaiveDateTime::parse_from_str(
        &created_at.chars().take(19).collect::<String>(),
        "%Y-%m-%d %H:%M:%S",
    ) else {
        return true;
    };
    let created = naive.and_utc();
    let age = chrono::Utc::now().signed_duration_since(created);
    age.num_seconds() >= STALE_CLAIM_SECS
}
