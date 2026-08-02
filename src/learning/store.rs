use crate::db::{db_execute, db_fetch_all, db_scalar, DbPool};
use anyhow::Result;
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

/// Persistent store for feedback learning (shared Postgres pool).
pub struct LearningStore {
    pool: DbPool,
}

impl LearningStore {
    pub fn from_pool(pool: &DbPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub fn dismiss(&self, finding: &Finding) -> Result<()> {
        block_on(self.dismiss_async(finding))
    }

    pub async fn dismiss_async(&self, finding: &Finding) -> Result<()> {
        self.dismiss_with_meta(finding, None, None, false).await
    }

    pub async fn dismiss_with_meta(
        &self,
        finding: &Finding,
        pr_number: Option<i64>,
        dismissed_by: Option<&str>,
        is_maintainer: bool,
    ) -> Result<()> {
        let fingerprint = finding.fingerprint();
        let repo: Option<String> = None;
        let by = dismissed_by.map(str::to_string);
        db_execute!(
            &self.pool,
            "INSERT INTO dismissed_findings (fingerprint, detector, file, line, message, repo_full_name, pr_number, dismissed_by, is_maintainer)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(fingerprint) DO UPDATE SET
               detector = excluded.detector,
               file = excluded.file,
               line = excluded.line,
               message = excluded.message,
               repo_full_name = COALESCE(excluded.repo_full_name, dismissed_findings.repo_full_name),
               pr_number = COALESCE(excluded.pr_number, dismissed_findings.pr_number),
               dismissed_by = COALESCE(excluded.dismissed_by, dismissed_findings.dismissed_by),
               is_maintainer = dismissed_findings.is_maintainer OR excluded.is_maintainer",
            &fingerprint,
            &finding.detector,
            &finding.file,
            finding.line as i64,
            &finding.message,
            &repo,
            &pr_number,
            &by,
            is_maintainer
        )?;
        crate::metrics::record_dismissal();
        let _ = crate::learning::mine::promote_dismissal_to_rule(
            self,
            &finding.detector,
            &finding.file,
            &finding.message,
            None,
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
        self.dismiss_fingerprint_for_repo(
            fingerprint,
            detector,
            file,
            message,
            None,
            None,
            None,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn dismiss_fingerprint_for_repo(
        &self,
        fingerprint: &str,
        detector: &str,
        file: &str,
        message: &str,
        repo_full_name: Option<&str>,
        pr_number: Option<i64>,
        dismissed_by: Option<&str>,
        is_maintainer: bool,
    ) -> Result<()> {
        let repo = repo_full_name.map(str::to_string);
        let by = dismissed_by.map(str::to_string);
        db_execute!(
            &self.pool,
            "INSERT INTO dismissed_findings (fingerprint, detector, file, line, message, repo_full_name, pr_number, dismissed_by, is_maintainer)
             VALUES (?, ?, ?, 0, ?, ?, ?, ?, ?)
             ON CONFLICT(fingerprint) DO UPDATE SET
               detector = excluded.detector,
               file = excluded.file,
               message = excluded.message,
               repo_full_name = COALESCE(excluded.repo_full_name, dismissed_findings.repo_full_name),
               pr_number = COALESCE(excluded.pr_number, dismissed_findings.pr_number),
               dismissed_by = COALESCE(excluded.dismissed_by, dismissed_findings.dismissed_by),
               is_maintainer = dismissed_findings.is_maintainer OR excluded.is_maintainer",
            fingerprint,
            detector,
            file,
            message,
            &repo,
            &pr_number,
            &by,
            is_maintainer
        )?;
        crate::metrics::record_dismissal();
        let _ = crate::learning::mine::promote_dismissal_to_rule(
            self,
            detector,
            file,
            message,
            repo_full_name,
        )
        .await;
        Ok(())
    }

    pub async fn un_dismiss_fingerprint(&self, fingerprint: &str) -> Result<bool> {
        Ok(db_execute!(
            &self.pool,
            "DELETE FROM dismissed_findings WHERE fingerprint = ?",
            fingerprint
        )? > 0)
    }

    pub async fn count_dismissals_for_detector(&self, detector: &str) -> Result<i64> {
        Ok(db_scalar!(
            &self.pool,
            i64,
            "SELECT COUNT(*) FROM dismissed_findings WHERE detector = ?",
            detector
        )?)
    }

    pub async fn count_distinct_prs_for_detector(
        &self,
        detector: &str,
        repo_full_name: Option<&str>,
    ) -> Result<i64> {
        if let Some(repo) = repo_full_name.filter(|r| !r.is_empty()) {
            Ok(db_scalar!(
                &self.pool,
                i64,
                "SELECT COUNT(DISTINCT pr_number) FROM dismissed_findings
                 WHERE detector = ? AND pr_number IS NOT NULL AND repo_full_name = ?",
                detector,
                repo
            )?)
        } else {
            Ok(db_scalar!(
                &self.pool,
                i64,
                "SELECT COUNT(DISTINCT pr_number) FROM dismissed_findings
                 WHERE detector = ? AND pr_number IS NOT NULL
                   AND (repo_full_name IS NULL OR repo_full_name = '')",
                detector
            )?)
        }
    }

    pub async fn count_maintainer_dismissals_for_detector(
        &self,
        detector: &str,
        repo_full_name: Option<&str>,
    ) -> Result<i64> {
        if let Some(repo) = repo_full_name.filter(|r| !r.is_empty()) {
            Ok(db_scalar!(
                &self.pool,
                i64,
                "SELECT COUNT(*) FROM dismissed_findings
                 WHERE detector = ? AND is_maintainer = TRUE AND repo_full_name = ?",
                detector,
                repo
            )?)
        } else {
            Ok(db_scalar!(
                &self.pool,
                i64,
                "SELECT COUNT(*) FROM dismissed_findings
                 WHERE detector = ? AND is_maintainer = TRUE
                   AND (repo_full_name IS NULL OR repo_full_name = '')",
                detector
            )?)
        }
    }

    pub fn add_rule(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        block_on(self.add_rule_async(rule))
    }

    pub async fn add_rule_async(&self, rule: &crate::learning::LearnedRule) -> Result<()> {
        let repo = rule.repo_full_name.clone();
        db_execute!(
            &self.pool,
            "INSERT INTO learned_rules (id, detector, file_pattern, message_pattern, action, reason, repo_full_name, status, source_count)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               detector = excluded.detector,
               file_pattern = excluded.file_pattern,
               message_pattern = excluded.message_pattern,
               action = excluded.action,
               reason = excluded.reason,
               repo_full_name = COALESCE(excluded.repo_full_name, learned_rules.repo_full_name),
               status = excluded.status,
               source_count = excluded.source_count",
            &rule.id,
            &rule.detector,
            &rule.file_pattern,
            &rule.message_pattern,
            rule.action.as_str(),
            &rule.reason,
            &repo,
            &rule.status,
            rule.source_count
        )?;
        Ok(())
    }

    /// Move a suggested rule into active use.
    pub async fn approve_rule(&self, id: &str) -> Result<bool> {
        let n = db_execute!(
            &self.pool,
            "UPDATE learned_rules SET status = 'approved', approved_at = NOW()
             WHERE id = ? AND status = 'suggested'",
            id
        )?;
        Ok(n > 0)
    }

    /// Retire a rule that no longer matches team behavior.
    pub async fn archive_rule(&self, id: &str) -> Result<bool> {
        let n = db_execute!(
            &self.pool,
            "UPDATE learned_rules SET status = 'archived', archived_at = NOW()
             WHERE id = ? AND status IN ('suggested', 'approved')",
            id
        )?;
        Ok(n > 0)
    }

    /// First approved rule matching detector + file path (decay check).
    pub async fn find_approved_rule_for_detector(
        &self,
        detector: &str,
        file: &str,
        repo_full_name: Option<&str>,
    ) -> Result<Option<String>> {
        let repo = repo_full_name.unwrap_or("");
        let row: Option<(String,)> = db_fetch_all!(
            &self.pool,
            (String,),
            "SELECT id FROM learned_rules
             WHERE detector = ? AND status = 'approved'
               AND (repo_full_name IS NULL OR repo_full_name = '' OR repo_full_name = ?)
               AND (file_pattern IS NULL OR ? LIKE file_pattern || '%')
             ORDER BY source_count DESC LIMIT 1",
            detector,
            repo,
            file
        )?
        .into_iter()
        .next();
        Ok(row.map(|(id,)| id))
    }

    pub async fn list_rules(&self) -> Result<Vec<crate::learning::LearnedRule>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: String,
            detector: String,
            file_pattern: Option<String>,
            message_pattern: Option<String>,
            action: String,
            reason: String,
            created_at: chrono::DateTime<chrono::Utc>,
            repo_full_name: Option<String>,
            status: String,
            source_count: i64,
        }
        let rows: Vec<Row> = db_fetch_all!(
            &self.pool,
            Row,
            "SELECT id, detector, file_pattern, message_pattern, action, reason, created_at, repo_full_name, status, source_count
             FROM learned_rules ORDER BY created_at DESC LIMIT 200"
        )?;
        Ok(rows
            .into_iter()
            .map(|r| crate::learning::LearnedRule {
                id: r.id,
                detector: r.detector,
                file_pattern: r.file_pattern,
                message_pattern: r.message_pattern,
                action: crate::learning::RuleAction::from_static_str(&r.action)
                    .unwrap_or(crate::learning::RuleAction::Ignore),
                reason: r.reason,
                created_at: r.created_at,
                repo_full_name: r.repo_full_name,
                status: r.status,
                source_count: r.source_count,
            })
            .collect())
    }

    pub async fn delete_rule(&self, id: &str) -> Result<bool> {
        let n = db_execute!(&self.pool, "DELETE FROM learned_rules WHERE id = ?", id)?;
        Ok(n > 0)
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

    pub fn filter_findings(
        &self,
        findings: &[Finding],
        repo: Option<&str>,
    ) -> Result<Vec<Finding>> {
        block_on(self.filter_findings_async(findings, repo))
    }

    pub async fn filter_findings_async(
        &self,
        findings: &[Finding],
        repo: Option<&str>,
    ) -> Result<Vec<Finding>> {
        if findings.is_empty() {
            return Ok(Vec::new());
        }

        let repo_scope = repo.map(str::to_string);
        let fingerprints: Vec<String> = findings.iter().map(|f| f.fingerprint()).collect();
        let mut dismissed_set = std::collections::HashSet::new();

        for chunk in fingerprints.chunks(400) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = if repo_scope.is_some() {
                format!(
                    "SELECT fingerprint FROM dismissed_findings
                     WHERE fingerprint IN ({placeholders})
                       AND (repo_full_name IS NULL OR repo_full_name = '' OR repo_full_name = ?)"
                )
            } else {
                format!(
                    "SELECT fingerprint FROM dismissed_findings
                     WHERE fingerprint IN ({placeholders})
                       AND (repo_full_name IS NULL OR repo_full_name = '')"
                )
            };
            let prepared = self.pool.prepare_sql(&sql);
            let mut q = sqlx::query_as::<_, (String,)>(&prepared);
            for fp in chunk {
                q = q.bind(fp);
            }
            if let Some(ref repo) = repo_scope {
                q = q.bind(repo);
            }
            let rows: Vec<(String,)> = q.fetch_all(self.pool.as_pg()).await?;
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
        let rules: Vec<Rule> = if let Some(ref repo) = repo_scope {
            db_fetch_all!(
                &self.pool,
                (String, Option<String>, Option<String>, String),
                "SELECT detector, file_pattern, message_pattern, action FROM learned_rules
                 WHERE status = 'approved'
                   AND (repo_full_name IS NULL OR repo_full_name = '' OR repo_full_name = ?)",
                repo
            )?
        } else {
            db_fetch_all!(
                &self.pool,
                (String, Option<String>, Option<String>, String),
                "SELECT detector, file_pattern, message_pattern, action FROM learned_rules
                 WHERE status = 'approved'
                   AND (repo_full_name IS NULL OR repo_full_name = '')"
            )?
        }
        .into_iter()
        .map(|(detector, file_pattern, message_pattern, action)| Rule {
            detector,
            file_pattern,
            message_pattern,
            action,
        })
        .collect();

        let short_prefixes: Vec<String> = if let Some(ref repo) = repo_scope {
            db_fetch_all!(
                &self.pool,
                (String,),
                "SELECT fingerprint FROM dismissed_findings
                 WHERE length(fingerprint) BETWEEN 12 AND 63
                   AND (repo_full_name IS NULL OR repo_full_name = '' OR repo_full_name = ?)",
                repo
            )?
            .into_iter()
            .map(|(fp,)| fp)
            .collect()
        } else {
            db_fetch_all!(
                &self.pool,
                (String,),
                "SELECT fingerprint FROM dismissed_findings
                 WHERE length(fingerprint) BETWEEN 12 AND 63
                   AND (repo_full_name IS NULL OR repo_full_name = '')"
            )?
            .into_iter()
            .map(|(fp,)| fp)
            .collect()
        };

        Ok(findings
            .iter()
            .filter_map(|f| {
                let fp = f.fingerprint();
                if dismissed_set.contains(&fp) {
                    return None;
                }
                if short_prefixes.iter().any(|p| {
                    // Prefix-of-fingerprint only; bidirectional match collided on short strings.
                    p.len() >= 12 && fp.starts_with(p.as_str())
                }) {
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
