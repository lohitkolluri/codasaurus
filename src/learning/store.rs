use anyhow::Result;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::LazyLock;
use tokio::runtime::{Handle, Runtime};

use crate::detectors::Finding;

static FALLBACK_RT: LazyLock<Runtime> =
    LazyLock::new(|| Runtime::new().expect("failed to create fallback tokio runtime"));

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    match Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => FALLBACK_RT.block_on(fut),
    }
}

/// Persistent store for feedback learning
pub struct LearningStore {
    pool: SqlitePool,
}

impl LearningStore {
    pub fn from_pool(pool: &crate::db::DbPool) -> Self {
        Self {
            pool: pool.0.clone(),
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
        let pool = block_on(SqlitePool::connect(&url))?;
        let store = Self { pool };
        store.initialize()?;
        Ok(store)
    }

    fn db_path() -> Result<PathBuf> {
        Ok(crate::storage::data_dir().join("learnings.db"))
    }

    fn initialize(&self) -> Result<()> {
        block_on(async {
            sqlx::query("PRAGMA journal_mode = WAL")
                .execute(&self.pool)
                .await?;
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS dismissed_findings (
                    fingerprint TEXT PRIMARY KEY,
                    detector TEXT NOT NULL,
                    file TEXT NOT NULL,
                    line INTEGER NOT NULL,
                    message TEXT NOT NULL,
                    dismissed_at TEXT NOT NULL DEFAULT (datetime('now'))
                 )",
            )
            .execute(&self.pool)
            .await?;
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS learned_rules (
                    id TEXT PRIMARY KEY,
                    detector TEXT NOT NULL,
                    file_pattern TEXT,
                    message_pattern TEXT,
                    action TEXT NOT NULL DEFAULT 'ignore',
                    reason TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 )",
            )
            .execute(&self.pool)
            .await?;
            Ok::<_, anyhow::Error>(())
        })
    }

    pub fn dismiss(&self, finding: &Finding) -> Result<()> {
        block_on(self.dismiss_async(finding))
    }

    pub async fn dismiss_async(&self, finding: &Finding) -> Result<()> {
        let fingerprint = finding.fingerprint();
        sqlx::query(
            "INSERT OR IGNORE INTO dismissed_findings (fingerprint, detector, file, line, message)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&fingerprint)
        .bind(&finding.detector)
        .bind(&finding.file)
        .bind(finding.line as i64)
        .bind(&finding.message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Dismiss by fingerprint string (used by bot `@codasaurus ignore <fp>`).
    pub async fn dismiss_fingerprint(
        &self,
        fingerprint: &str,
        detector: &str,
        file: &str,
        message: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO dismissed_findings (fingerprint, detector, file, line, message)
             VALUES (?, ?, ?, 0, ?)",
        )
        .bind(fingerprint)
        .bind(detector)
        .bind(file)
        .bind(message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub fn add_rule(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        block_on(async {
            sqlx::query(
                "INSERT OR REPLACE INTO learned_rules (id, detector, file_pattern, message_pattern, action, reason)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&rule.id)
            .bind(&rule.detector)
            .bind(&rule.file_pattern)
            .bind(&rule.message_pattern)
            .bind(rule.action.as_str())
            .bind(&rule.reason)
            .execute(&self.pool)
            .await
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub fn clear_for_test(&self) -> Result<()> {
        block_on(async {
            sqlx::query("DELETE FROM dismissed_findings")
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM learned_rules")
                .execute(&self.pool)
                .await
        })?;
        Ok(())
    }

    pub fn filter_findings(&self, findings: &[Finding]) -> Result<Vec<Finding>> {
        block_on(self.filter_findings_async(findings))
    }

    pub async fn filter_findings_async(&self, findings: &[Finding]) -> Result<Vec<Finding>> {
        if findings.is_empty() {
            return Ok(Vec::new());
        }

        let fingerprints: Vec<String> = findings.iter().map(|f| f.fingerprint()).collect();
        let mut dismissed_set = std::collections::HashSet::new();

        // Query only fingerprints present in this review (avoids full-table scan as dismissals grow).
        // SQLite binds are capped; chunk to stay safe.
        for chunk in fingerprints.chunks(400) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT fingerprint FROM dismissed_findings WHERE fingerprint IN ({placeholders})"
            );
            let mut q = sqlx::query_as::<_, (String,)>(&sql);
            for fp in chunk {
                q = q.bind(fp);
            }
            let rows = q.fetch_all(&self.pool).await?;
            for (fp,) in rows {
                dismissed_set.insert(fp);
            }
        }

        struct Rule {
            detector: String,
            file_pattern: Option<String>,
            message_pattern: Option<String>,
        }
        let rules: Vec<Rule> = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT detector, file_pattern, message_pattern FROM learned_rules",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(detector, file_pattern, message_pattern)| Rule {
            detector,
            file_pattern,
            message_pattern,
        })
        .collect();

        Ok(findings
            .iter()
            .filter(|f| {
                let fp = f.fingerprint();
                if dismissed_set.contains(&fp) {
                    return false;
                }
                for rule in &rules {
                    if rule.detector != f.detector {
                        continue;
                    }
                    if let Some(ref pat) = rule.file_pattern {
                        if !f.file.contains(pat) {
                            continue;
                        }
                    }
                    if let Some(ref pat) = rule.message_pattern {
                        if !f.message.contains(pat) {
                            continue;
                        }
                    }
                    return false;
                }
                true
            })
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::Finding;

    #[test]
    fn test_learning_store_open() {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open_at(dir.path().join("learnings.db"));
        assert!(store.is_ok(), "learning store should open");
    }

    #[test]
    fn test_dismiss_and_filter() {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open_at(dir.path().join("learnings.db")).unwrap();

        let finding = Finding {
            detector: "test-detector".to_string(),
            severity: "warning",
            file: "test.rs".to_string(),
            line: 10,
            column: 0,
            message: "test finding".to_string(),
            suggestion: None,
            evidence: None,
            codemod: None,
        };

        let result = store
            .filter_findings(std::slice::from_ref(&finding))
            .unwrap();
        assert_eq!(result.len(), 1);

        store.dismiss(&finding).unwrap();

        let result = store.filter_findings(&[finding]).unwrap();
        assert_eq!(result.len(), 0);
    }
}
