use anyhow::Result;
use crate::db::{db_execute, db_fetch_all, db_scalar, DbPool};
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
    pool: DbPool,
}

impl LearningStore {
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
        Ok(Self { pool })
    }

    fn db_path() -> Result<PathBuf> {
        Ok(crate::storage::data_dir().join("learnings.db"))
    }

    pub fn dismiss(&self, finding: &Finding) -> Result<()> {
        block_on(self.dismiss_async(finding))
    }

    pub async fn dismiss_async(&self, finding: &Finding) -> Result<()> {
        let fingerprint = finding.fingerprint();
        db_execute!(
            &self.pool,
            "INSERT INTO dismissed_findings (fingerprint, detector, file, line, message)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(fingerprint) DO NOTHING",
            &fingerprint,
            &finding.detector,
            &finding.file,
            finding.line as i64,
            &finding.message
        )?;
        crate::metrics::record_dismissal();
        let _ = crate::learning::mine::promote_dismissal_to_rule(
            self,
            &finding.detector,
            &finding.file,
            &finding.message,
        )
        .await;
        Ok(())
    }

    pub async fn dismiss_fingerprint(
        &self,
        fingerprint: &str,
        detector: &str,
        file: &str,
        message: &str,
    ) -> Result<()> {
        db_execute!(
            &self.pool,
            "INSERT INTO dismissed_findings (fingerprint, detector, file, line, message)
             VALUES (?, ?, ?, 0, ?)
             ON CONFLICT(fingerprint) DO NOTHING",
            fingerprint,
            detector,
            file,
            message
        )?;
        crate::metrics::record_dismissal();
        let _ = crate::learning::mine::promote_dismissal_to_rule(self, detector, file, message)
            .await;
        Ok(())
    }

    pub fn add_rule(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        block_on(self.add_rule_async(rule))
    }

    pub async fn add_rule_async(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        db_execute!(
            &self.pool,
            "INSERT INTO learned_rules (id, detector, file_pattern, message_pattern, action, reason)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               detector = excluded.detector,
               file_pattern = excluded.file_pattern,
               message_pattern = excluded.message_pattern,
               action = excluded.action,
               reason = excluded.reason",
            &rule.id,
            &rule.detector,
            &rule.file_pattern,
            &rule.message_pattern,
            rule.action.as_str(),
            &rule.reason
        )?;
        Ok(())
    }

    pub async fn count_dismissals_for_detector(&self, detector: &str) -> Result<i64> {
        Ok(db_scalar!(
            &self.pool,
            i64,
            "SELECT COUNT(*) FROM dismissed_findings WHERE detector = ?",
            detector
        )?)
    }

    pub async fn count_dismissals_total(&self) -> Result<i64> {
        Ok(db_scalar!(
            &self.pool,
            i64,
            "SELECT COUNT(*) FROM dismissed_findings"
        )?)
    }

    #[cfg(test)]
    pub fn clear_for_test(&self) -> Result<()> {
        block_on(async {
            db_execute!(&self.pool, "DELETE FROM dismissed_findings")?;
            db_execute!(&self.pool, "DELETE FROM learned_rules")?;
            Ok::<_, sqlx::Error>(())
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

        for chunk in fingerprints.chunks(400) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT fingerprint FROM dismissed_findings WHERE fingerprint IN ({placeholders})"
            );
            let prepared = self.pool.prepare_sql(&sql);
            let rows: Vec<(String,)> = match &self.pool {
                DbPool::Sqlite(p) => {
                    let mut q = sqlx::query_as::<_, (String,)>(&prepared);
                    for fp in chunk {
                        q = q.bind(fp);
                    }
                    q.fetch_all(p).await?
                }
                DbPool::Postgres(p) => {
                    let mut q = sqlx::query_as::<_, (String,)>(&prepared);
                    for fp in chunk {
                        q = q.bind(fp);
                    }
                    q.fetch_all(p).await?
                }
            };
            for (fp,) in rows {
                dismissed_set.insert(fp);
            }
        }

        struct Rule {
            detector: String,
            file_pattern: Option<String>,
            message_pattern: Option<String>,
            action: String,
        }
        let rules: Vec<Rule> = db_fetch_all!(
            &self.pool,
            (String, Option<String>, Option<String>, String),
            "SELECT detector, file_pattern, message_pattern, action FROM learned_rules"
        )?
        .into_iter()
        .map(|(detector, file_pattern, message_pattern, action)| Rule {
            detector,
            file_pattern,
            message_pattern,
            action,
        })
        .collect();

        let short_sql =
            "SELECT fingerprint FROM dismissed_findings WHERE length(fingerprint) BETWEEN 8 AND 63";
        let short_prefixes: Vec<String> = db_fetch_all!(&self.pool, (String,), short_sql)?
            .into_iter()
            .map(|(fp,)| fp)
            .collect();

        Ok(findings
            .iter()
            .filter_map(|f| {
                let fp = f.fingerprint();
                if dismissed_set.contains(&fp) {
                    return None;
                }
                if short_prefixes
                    .iter()
                    .any(|p| fp.starts_with(p.as_str()) || p.starts_with(fp.as_str()))
                {
                    return None;
                }

                let mut out = f.clone();
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
                    match rule.action.as_str() {
                        "ignore" => return None,
                        "downgrade" => {
                            out.severity = match out.severity {
                                "blocking" => "warning",
                                "warning" => "info",
                                other => other,
                            };
                        }
                        "always_warn" => {
                            if out.severity == "info" {
                                out.severity = "warning";
                            }
                        }
                        _ => return None,
                    }
                }
                Some(out)
            })
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
