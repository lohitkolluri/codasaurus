use anyhow::Result;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tokio::runtime::Runtime;

use crate::detectors::Finding;

/// Persistent store for feedback learning
pub struct LearningStore {
    pool: SqlitePool,
    rt: Runtime,
}

impl LearningStore {
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
        Ok(crate::storage::data_dir().join("learnings.db"))
    }

    fn initialize(&self) -> Result<()> {
        self.rt.block_on(async {
            sqlx::query("PRAGMA journal_mode = WAL")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA synchronous = NORMAL")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA cache_size = -64000")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA mmap_size = 268435456")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA temp_store = MEMORY")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA busy_timeout = 5000")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA auto_vacuum = INCREMENTAL")
                .execute(&self.pool)
                .await?;
            sqlx::query("PRAGMA foreign_keys = ON")
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
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_dismissed_fingerprint ON dismissed_findings(fingerprint)",
            )
            .execute(&self.pool)
            .await?;
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_learned_detector ON learned_rules(detector)",
            )
            .execute(&self.pool)
            .await
        })?;
        Ok(())
    }

    pub fn dismiss(&self, finding: &Finding) -> Result<()> {
        let fingerprint = finding.fingerprint();
        self.rt.block_on(async {
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
            .await
        })?;
        Ok(())
    }

    pub fn add_rule(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        self.rt.block_on(async {
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
        self.rt.block_on(async {
            sqlx::query("DELETE FROM dismissed_findings")
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM learned_rules")
                .execute(&self.pool)
                .await
        })?;
        Ok(())
    }

    /// Batch filter: loads all dismissed fingerprints + all learned rules into memory via 2 queries,
    /// then checks each finding against the in-memory sets. Avoids 2N individual SQL queries.
    pub fn filter_findings(&self, findings: &[Finding]) -> Result<Vec<Finding>> {
        if findings.is_empty() {
            return Ok(Vec::new());
        }

        let (dismissed_set, rules) = self.rt.block_on(async {
            // Query 1: Load all dismissed fingerprints into a HashSet
            let mut dismissed_set = std::collections::HashSet::new();
            {
                let rows: Vec<(String,)> =
                    sqlx::query_as("SELECT fingerprint FROM dismissed_findings")
                        .fetch_all(&self.pool)
                        .await?;
                for (fp,) in rows {
                    dismissed_set.insert(fp);
                }
            }

            // Query 2: Load all learned rules into memory
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

            Ok::<_, anyhow::Error>((dismissed_set, rules))
        })?;

        // In-memory batch check: HashSet membership + rule matching
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

        // Before dismissal, filter should include it
        let result = store
            .filter_findings(std::slice::from_ref(&finding))
            .unwrap();
        assert_eq!(result.len(), 1, "should include finding before dismissal");

        // Dismiss it
        store.dismiss(&finding).unwrap();

        // After dismissal, filter should exclude it
        let result = store.filter_findings(&[finding]).unwrap();
        assert_eq!(result.len(), 0, "should exclude dismissed finding");
    }
}
