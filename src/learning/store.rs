use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::detectors::Finding;

/// Persistent store for feedback learning
pub struct LearningStore {
    conn: Mutex<Connection>,
}

impl LearningStore {
    pub fn open() -> Result<Self> {
        Self::open_at(Self::db_path()?)
    }

    fn open_at(path: PathBuf) -> Result<Self> {
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
        Ok(crate::storage::data_dir().join("learnings.db"))
    }

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
             PRAGMA cache_size = -64000;
             PRAGMA mmap_size = 268435456;
             PRAGMA temp_store = MEMORY;
             PRAGMA busy_timeout = 5000;
             PRAGMA auto_vacuum = INCREMENTAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS dismissed_findings (
                fingerprint TEXT PRIMARY KEY,
                detector TEXT NOT NULL,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                message TEXT NOT NULL,
                dismissed_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS learned_rules (
                id TEXT PRIMARY KEY,
                detector TEXT NOT NULL,
                file_pattern TEXT,
                message_pattern TEXT,
                action TEXT NOT NULL DEFAULT 'ignore',
                reason TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE INDEX IF NOT EXISTS idx_dismissed_fingerprint ON dismissed_findings(fingerprint);
             CREATE INDEX IF NOT EXISTS idx_learned_detector ON learned_rules(detector);",
        )?;
        Ok(())
    }

    pub fn dismiss(&self, finding: &Finding) -> Result<()> {
        let fingerprint = finding.fingerprint();
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO dismissed_findings (fingerprint, detector, file, line, message)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                fingerprint,
                finding.detector,
                finding.file,
                finding.line as i64,
                finding.message
            ],
        )?;
        Ok(())
    }

    pub fn add_rule(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR REPLACE INTO learned_rules (id, detector, file_pattern, message_pattern, action, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                rule.id, rule.detector, rule.file_pattern,
                rule.message_pattern, rule.action.as_str(), rule.reason
            ],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn clear_for_test(&self) -> Result<()> {
        let conn = self.lock();
        conn.execute_batch("DELETE FROM dismissed_findings; DELETE FROM learned_rules;")?;
        Ok(())
    }

    /// Batch filter: loads all dismissed fingerprints + all learned rules into memory via 2 queries,
    /// then checks each finding against the in-memory sets. Avoids 2N individual SQL queries.
    pub fn filter_findings(&self, findings: &[Finding]) -> Result<Vec<Finding>> {
        if findings.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.lock();

        // Query 1: Load all dismissed fingerprints into a HashSet (single query)
        let mut dismissed_set = std::collections::HashSet::new();
        {
            let mut stmt = conn.prepare_cached("SELECT fingerprint FROM dismissed_findings")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for fp in rows.flatten() {
                dismissed_set.insert(fp);
            }
        }

        // Query 2: Load all learned rules into memory (single query)
        struct Rule {
            detector: String,
            file_pattern: Option<String>,
            message_pattern: Option<String>,
        }
        let mut rules: Vec<Rule> = Vec::new();
        {
            let mut stmt = conn.prepare_cached(
                "SELECT detector, file_pattern, message_pattern FROM learned_rules",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Rule {
                    detector: row.get(0)?,
                    file_pattern: row.get(1)?,
                    message_pattern: row.get(2)?,
                })
            })?;
            for rule in rows.flatten() {
                rules.push(rule);
            }
        }
        drop(conn);

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
