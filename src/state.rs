//! Lightweight state store for tracking review comment IDs and commit SHAs.
//! Keyed by `{repo_owner}/{repo_name}/{pr_number}`.
//! Uses SQLite with WAL mode for persistence across bot restarts.

use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

pub struct ReviewState {
    conn: Mutex<Connection>,
}

impl ReviewState {
    pub fn open() -> Result<Self> {
        let path = Self::db_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.initialize()?;
        Ok(store)
    }

    fn db_path() -> Result<PathBuf> {
        let home = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        Ok(home.join("codasaurus").join("review_state.db"))
    }

    /// Lock the connection, recovering from a poisoned mutex if needed.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| {
            eprintln!("Warning: DB connection mutex poisoned, recovering");
            e.into_inner()
        })
    }

    fn initialize(&self) -> Result<()> {
        let conn = self.lock();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -8000;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS review_comments (
                repo_pr TEXT PRIMARY KEY,
                comment_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS reviewed_commits (
                repo_pr TEXT PRIMARY KEY,
                head_sha TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )?;
        Ok(())
    }

    /// Get the stored comment ID for a repo+PR combination.
    pub fn get_comment_id(&self, repo: &str, pr_number: i64) -> Result<Option<i64>> {
        let conn = self.lock();
        let key = format!("{}/{}", repo, pr_number);
        let mut stmt = conn.prepare_cached(
            "SELECT comment_id FROM review_comments WHERE repo_pr = ?1",
        )?;
        let mut rows = stmt.query_map([&key], |row| row.get::<_, i64>(0))?;
        match rows.next() {
            Some(Ok(id)) => Ok(Some(id)),
            _ => Ok(None),
        }
    }

    /// Get the last reviewed commit SHA for a repo+PR combination.
    pub fn get_reviewed_sha(&self, repo: &str, pr_number: i64) -> Result<Option<String>> {
        let conn = self.lock();
        let key = format!("{}/{}", repo, pr_number);
        let mut stmt = conn.prepare_cached(
            "SELECT head_sha FROM reviewed_commits WHERE repo_pr = ?1",
        )?;
        let mut rows = stmt.query_map([&key], |row| row.get::<_, String>(0))?;
        match rows.next() {
            Some(Ok(sha)) => Ok(Some(sha)),
            _ => Ok(None),
        }
    }

    /// Store or update the last reviewed commit SHA.
    pub fn set_reviewed_sha(&self, repo: &str, pr_number: i64, sha: &str) -> Result<()> {
        let conn = self.lock();
        let key = format!("{}/{}", repo, pr_number);
        conn.execute(
            "INSERT OR REPLACE INTO reviewed_commits (repo_pr, head_sha) VALUES (?1, ?2)",
            rusqlite::params![key, sha],
        )?;
        Ok(())
    }

    /// Store or update the comment ID for a repo+PR combination.
    pub fn set_comment_id(&self, repo: &str, pr_number: i64, comment_id: i64) -> Result<()> {
        let conn = self.lock();
        let key = format!("{}/{}", repo, pr_number);
        conn.execute(
            "INSERT OR REPLACE INTO review_comments (repo_pr, comment_id) VALUES (?1, ?2)",
            rusqlite::params![key, comment_id],
        )?;
        Ok(())
    }
}
