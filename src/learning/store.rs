use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::detectors::Finding;

/// Persistent store for feedback learning
pub struct LearningStore {
    conn: Mutex<Connection>,
}

impl LearningStore {
    pub fn open() -> Result<Self> {
        let path = Self::db_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&path)?;
        let store = Self { conn: Mutex::new(conn) };
        store.initialize()?;
        Ok(store)
    }

    fn db_path() -> Result<PathBuf> {
        let home = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(home.join("codasaurus").join("learnings.db"))
    }

    fn initialize(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dismissed_findings (
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
            CREATE INDEX IF NOT EXISTS idx_learned_detector ON learned_rules(detector);"
        )?;
        Ok(())
    }

    pub fn is_dismissed(&self, finding: &Finding) -> Result<bool> {
        let fingerprint = finding.fingerprint();
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM dismissed_findings WHERE fingerprint = ?1",
                [&fingerprint],
                |row| row.get(0),
            )
            .unwrap_or(false);
        let ruled_out: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM learned_rules
                 WHERE detector = ?1
                 AND (file_pattern IS NULL OR ?2 LIKE file_pattern)
                 AND (message_pattern IS NULL OR ?3 LIKE message_pattern)",
                rusqlite::params![finding.detector, finding.file, finding.message],
                |row| row.get(0),
            )
            .unwrap_or(false);
        Ok(exists || ruled_out)
    }

    pub fn dismiss(&self, finding: &Finding) -> Result<()> {
        let fingerprint = finding.fingerprint();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO dismissed_findings (fingerprint, detector, file, line, message)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![fingerprint, finding.detector, finding.file, finding.line, finding.message],
        )?;
        Ok(())
    }

    pub fn add_rule(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO learned_rules (id, detector, file_pattern, message_pattern, action, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                rule.id, rule.detector, rule.file_pattern,
                rule.message_pattern, rule.action, rule.reason
            ],
        )?;
        Ok(())
    }

    pub fn filter_findings(&self, findings: Vec<Finding>) -> Result<Vec<Finding>> {
        Ok(findings
            .into_iter()
            .filter(|f| !self.is_dismissed(f).unwrap_or(false))
            .collect())
    }
}
