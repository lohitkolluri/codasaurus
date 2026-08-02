//! Review comment IDs and commit SHA tracking (multi-replica safe leases).

use crate::db::{db_execute, db_fetch_optional, db_scalar_optional, DbPool};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::sync::OnceLock;

/// Lease/stale window must exceed the review timeout or a second worker can
/// reclaim a still-running review (duplicate APPROVE / check runs).
fn stale_claim_secs() -> i64 {
    let timeout = crate::bot_runtime::BotRuntimeConfig::default().review_timeout_secs as i64;
    timeout.saturating_add(120).max(600)
}

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
        Self { pool: pool.clone() }
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

    pub async fn get_reviewed_sha_async(
        &self,
        repo: &str,
        pr_number: i64,
    ) -> Result<Option<String>> {
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
            (String, String, DateTime<Utc>, String),
            "SELECT head_sha,
                    COALESCE(status, 'completed'),
                    COALESCE(created_at, NOW()),
                    COALESCE(lease_owner, '')
             FROM reviewed_commits WHERE repo_pr = ?",
            &key
        )? {
            let (existing, status, created_at, lease_owner) = row;
            if existing == sha {
                if status == "completed" {
                    return Ok(false);
                }
                if status == "in_progress" && lease_owner != owner && !is_claim_stale(created_at) {
                    return Ok(false);
                }
                if status == "in_progress" && lease_owner == owner && !is_claim_stale(created_at) {
                    return Ok(false);
                }
            }
        }

        let result = sqlx::query(
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status, lease_owner, created_at)
             VALUES ($1, $2, 'in_progress', $3, NOW())
             ON CONFLICT(repo_pr) DO UPDATE SET
               head_sha = excluded.head_sha,
               status = 'in_progress',
               lease_owner = excluded.lease_owner,
               created_at = NOW()
             WHERE reviewed_commits.head_sha != excluded.head_sha
                OR reviewed_commits.status != 'in_progress'
                OR reviewed_commits.lease_owner = excluded.lease_owner
                OR reviewed_commits.created_at < NOW() - ($4::bigint * INTERVAL '1 second')",
        )
        .bind(&key)
        .bind(sha)
        .bind(owner)
        .bind(stale_claim_secs())
        .execute(self.pool.as_pg())
        .await?
        .rows_affected();

        Ok(result > 0)
    }

    /// Force-claim a SHA even when a completed review already exists (slash re-review).
    pub async fn force_claim_sha(&self, repo: &str, pr_number: i64, sha: &str) -> Result<bool> {
        let key = format!("{repo}/{pr_number}");
        let owner = lease_owner_id();
        let result = sqlx::query(
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status, lease_owner, created_at)
             VALUES ($1, $2, 'in_progress', $3, NOW())
             ON CONFLICT(repo_pr) DO UPDATE SET
               head_sha = excluded.head_sha,
               status = 'in_progress',
               lease_owner = excluded.lease_owner,
               created_at = NOW()",
        )
        .bind(&key)
        .bind(sha)
        .bind(owner)
        .execute(self.pool.as_pg())
        .await?
        .rows_affected();
        Ok(result > 0)
    }

    pub async fn set_reviewed_sha_async(
        &self,
        repo: &str,
        pr_number: i64,
        sha: &str,
    ) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        let owner = lease_owner_id();
        db_execute!(
            &self.pool,
            "INSERT INTO reviewed_commits (repo_pr, head_sha, status, lease_owner, created_at)
             VALUES (?, ?, 'completed', ?, NOW())
             ON CONFLICT(repo_pr) DO UPDATE SET
               head_sha = excluded.head_sha,
               status = 'completed',
               lease_owner = excluded.lease_owner,
               created_at = NOW()",
            &key,
            sha,
            owner
        )?;
        Ok(())
    }

    pub async fn release_sha_claim(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let key = format!("{repo}/{pr_number}");
        let owner = lease_owner_id();
        sqlx::query(
            "DELETE FROM reviewed_commits
             WHERE repo_pr = $1 AND head_sha = $2 AND status = 'in_progress'
               AND (lease_owner = $3 OR lease_owner = ''
                    OR created_at < NOW() - ($4::bigint * INTERVAL '1 second'))",
        )
        .bind(&key)
        .bind(sha)
        .bind(owner)
        .bind(stale_claim_secs())
        .execute(self.pool.as_pg())
        .await?;
        Ok(())
    }
}

fn is_claim_stale(created_at: DateTime<Utc>) -> bool {
    let age = Utc::now().signed_duration_since(created_at);
    age.num_seconds() >= stale_claim_secs()
}
