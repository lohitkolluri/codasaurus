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

/// Learning scope keys that apply to `repo_full_name`, broadest first:
/// global (`""`), org (`"owner/"`), repo (`"owner/repo"`).
///
/// The org key keeps a trailing slash so it can never collide with a real
/// `owner/repo` value. Before this tier existed the only shared scope was
/// instance-wide, so a rule learned in one GitHub org silently suppressed
/// findings in every other org installed on the same instance.
pub fn scope_chain(repo_full_name: Option<&str>) -> Vec<String> {
    let mut chain = vec![String::new()];
    let Some(repo) = repo_full_name.filter(|r| !r.is_empty()) else {
        return chain;
    };
    if let Some((owner, _)) = repo.split_once('/') {
        if !owner.is_empty() {
            chain.push(format!("{owner}/"));
        }
    }
    chain.push(repo.to_string());
    chain
}

/// Does `path` fall under a learned rule's `file_pattern`?
///
/// The old matcher was a plain `path.contains(pattern)`, which was neither
/// anchored nor segment-aware: a rule learned for `src/api` also silently
/// suppressed findings in `vendor/src/api-v2/` — files nobody ever dismissed.
///
/// Replacing it outright would have broken every hand-written rule already in
/// production (`migrations`, `node_modules`, `.generated.ts` all stop matching
/// under a strictly anchored rule), so this accepts the union of:
///
/// 1. a repo-rooted path prefix — `src/api` covers `src/api/auth.rs`, which is
///    the shape `mine::file_pattern_from_path` writes; and
/// 2. the same glob syntax as `checks.exclude_patterns`, so hand-written
///    suffix (`.generated.ts`), star (`*.rs`) and directory (`node_modules/`)
///    patterns keep working and users only learn one pattern language.
///
/// Form 2 is tried both as written and as a directory, so a bare `migrations`
/// still matches `db/migrations/x.sql` by whole path segment. What it no longer
/// does is match a *partial* segment, which is the bug above.
fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    let trimmed = pattern.trim_end_matches('/');
    if trimmed.is_empty() {
        return true;
    }
    // Form 1: anchored at the repo root, on a segment boundary.
    if path == trimmed
        || path
            .strip_prefix(trimmed)
            .is_some_and(|rest| rest.starts_with('/'))
    {
        return true;
    }
    // Form 2: the shared exclude-pattern syntax, as written and as a directory.
    let prepared =
        crate::detectors::prepare_exclude_patterns(&[pattern.to_string(), format!("{trimmed}/")]);
    crate::detectors::is_excluded_prepared(path, &prepared)
}

/// Persistent store for feedback learning (shared Postgres pool).
pub struct LearningStore {
    pool: DbPool,
}

impl LearningStore {
    pub fn from_pool(pool: &DbPool) -> Self {
        Self { pool: pool.clone() }
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
        let repo = repo_full_name.unwrap_or("").to_string();
        let by = dismissed_by.map(str::to_string);
        db_execute!(
            &self.pool,
            "INSERT INTO dismissed_findings (fingerprint, detector, file, line, message, repo_full_name, pr_number, dismissed_by, is_maintainer)
             VALUES (?, ?, ?, 0, ?, ?, ?, ?, ?)
             ON CONFLICT(repo_full_name, fingerprint) DO UPDATE SET
               detector = excluded.detector,
               file = excluded.file,
               message = excluded.message,
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

    pub async fn un_dismiss_fingerprint(
        &self,
        fingerprint: &str,
        repo_full_name: Option<&str>,
    ) -> Result<bool> {
        let repo = repo_full_name.unwrap_or("");
        Ok(db_execute!(
            &self.pool,
            "DELETE FROM dismissed_findings WHERE fingerprint = ? AND repo_full_name = ?",
            fingerprint,
            repo
        )? > 0)
    }

    /// Distinct PRs in **this repo alone** that dismissed `detector`.
    ///
    /// Deliberately not scope-chained. This count is the non-maintainer half of
    /// the auto-promotion guard in [`crate::learning::mine`], so widening it to
    /// the org would let dismissals spread across many repos clear a bar that is
    /// documented as "within the same repo" — and a rule promoted that way then
    /// suppresses findings for a repo that never dismissed anything.
    pub async fn count_distinct_prs_for_detector(
        &self,
        detector: &str,
        repo_full_name: Option<&str>,
    ) -> Result<i64> {
        let repo = repo_full_name.unwrap_or("");
        Ok(db_scalar!(
            &self.pool,
            i64,
            "SELECT COUNT(DISTINCT pr_number) FROM dismissed_findings
             WHERE detector = ? AND pr_number IS NOT NULL
               AND COALESCE(repo_full_name, '') = ?",
            detector,
            repo
        )?)
    }

    pub async fn count_maintainer_dismissals_for_detector(
        &self,
        detector: &str,
        repo_full_name: Option<&str>,
    ) -> Result<i64> {
        let scopes = scope_chain(repo_full_name);
        Ok(db_scalar!(
            &self.pool,
            i64,
            "SELECT COUNT(*) FROM dismissed_findings
             WHERE detector = ? AND is_maintainer = TRUE
               AND COALESCE(repo_full_name, '') = ANY(?)",
            detector,
            &scopes
        )?)
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

    /// The rule `filter_findings_async` would apply to `file` (decay check).
    ///
    /// Decay *archives* the rule it returns, so it must pick the same one the
    /// filter applied. The path match therefore runs through
    /// [`path_matches_pattern`] in Rust rather than being re-expressed in SQL:
    /// the SQL version silently disagreed with the filter (it had no `*`-glob
    /// branch, and a `%` or `_` inside a stored pattern became a LIKE wildcard
    /// that matched everything), so decay could archive a rule that had never
    /// fired while leaving the one that did suppressing findings.
    ///
    /// Ordering matches the filter's precedence for the same reason.
    pub async fn find_approved_rule_for_detector(
        &self,
        detector: &str,
        file: &str,
        repo_full_name: Option<&str>,
    ) -> Result<Option<String>> {
        let scopes = scope_chain(repo_full_name);
        let candidates: Vec<(String, Option<String>)> = db_fetch_all!(
            &self.pool,
            (String, Option<String>),
            "SELECT id, file_pattern FROM learned_rules
             WHERE detector = ? AND status = 'approved'
               AND COALESCE(repo_full_name, '') = ANY(?)
             ORDER BY length(COALESCE(repo_full_name, '')) DESC, source_count DESC, id",
            detector,
            &scopes
        )?;
        Ok(candidates
            .into_iter()
            .find(|(_, pattern)| {
                pattern
                    .as_deref()
                    .is_none_or(|pat| path_matches_pattern(file, pat))
            })
            .map(|(id, _)| id))
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
            match_count: i64,
            last_matched_at: Option<chrono::DateTime<chrono::Utc>>,
        }
        let rows: Vec<Row> = db_fetch_all!(
            &self.pool,
            Row,
            "SELECT id, detector, file_pattern, message_pattern, action, reason, created_at, repo_full_name, status, source_count,
                    match_count, last_matched_at
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
                match_count: r.match_count,
                last_matched_at: r.last_matched_at,
            })
            .collect())
    }

    pub async fn delete_rule(&self, id: &str) -> Result<bool> {
        let n = db_execute!(&self.pool, "DELETE FROM learned_rules WHERE id = ?", id)?;
        Ok(n > 0)
    }

    /// Recent maintainer dismissals in scope, as `(detector, message)`.
    ///
    /// Fed to the LLM as "this team has already rejected these" so it stops
    /// re-raising them. `filter_findings_async` only suppresses *detector*
    /// findings, so without this the LLM keeps paying tokens to rediscover
    /// judgement calls the maintainers already made.
    ///
    /// Maintainer-only on purpose: anyone with a 👎 can record a dismissal, and
    /// a non-maintainer must not be able to steer the model away from a class of
    /// finding. Capped because this goes into every prompt.
    pub async fn recent_maintainer_dismissals(
        &self,
        repo_full_name: Option<&str>,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        let scopes = scope_chain(repo_full_name);
        Ok(db_fetch_all!(
            &self.pool,
            (String, String),
            "SELECT DISTINCT ON (detector, message) detector, message
               FROM dismissed_findings
              WHERE is_maintainer = TRUE
                AND message <> ''
                AND COALESCE(repo_full_name, '') = ANY(?)
              ORDER BY detector, message
              LIMIT ?",
            &scopes,
            limit
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

        let scopes = scope_chain(repo);
        // Fingerprints are SHA-256 hex and cost real CPU; compute once and reuse
        // for the lookup, the prefix check, and the final filter.
        let fingerprints: Vec<String> = findings.iter().map(|f| f.fingerprint()).collect();

        // One array-bound query instead of chunked dynamically-built `IN (?,?,…)`.
        let dismissed_set: std::collections::HashSet<String> = db_fetch_all!(
            &self.pool,
            (String,),
            "SELECT fingerprint FROM dismissed_findings
             WHERE fingerprint = ANY(?) AND COALESCE(repo_full_name, '') = ANY(?)",
            &fingerprints,
            &scopes
        )?
        .into_iter()
        .map(|(fp,)| fp)
        .collect();

        struct Rule {
            id: String,
            file_pattern: Option<String>,
            message_pattern: Option<String>,
            action: String,
        }
        // Bucket rules by detector: the scan skipped on detector mismatch, so every
        // finding walked every rule in the scope.
        //
        // Precedence is decided here, in SQL, so it can't vary with row order:
        // scope specificity first, then how much evidence backs the rule, then id
        // as a final tiebreak.
        //
        // Specificity is ordered by scope *length*, not by the scope string. Each
        // broader scope is a strict prefix of the narrower one (`""` < `"owner/"` <
        // `"owner/repo"`), so length is exactly as precise — and unlike a string
        // sort it does not depend on the database's collation, which may ignore
        // punctuation such as `/` at the primary level.
        let mut rules_by_detector: std::collections::HashMap<String, Vec<Rule>> =
            std::collections::HashMap::new();
        for (id, detector, file_pattern, message_pattern, action) in db_fetch_all!(
            &self.pool,
            (String, String, Option<String>, Option<String>, String),
            "SELECT id, detector, file_pattern, message_pattern, action FROM learned_rules
             WHERE status = 'approved'
               AND COALESCE(repo_full_name, '') = ANY(?)
             ORDER BY length(COALESCE(repo_full_name, '')) DESC, source_count DESC, id",
            &scopes
        )? {
            rules_by_detector.entry(detector).or_default().push(Rule {
                id,
                file_pattern,
                message_pattern,
                action,
            });
        }

        // Truncated fingerprints stored by hand match any full fingerprint they
        // prefix. Bucketing by length turns the per-finding scan over every prefix
        // into one hash lookup per distinct prefix length (a handful, not hundreds).
        let mut prefixes_by_len: std::collections::HashMap<
            usize,
            std::collections::HashSet<String>,
        > = std::collections::HashMap::new();
        // Bounds are the fingerprint format, not a tuning knob: below the lower
        // bound a prefix is short enough to collide across unrelated findings;
        // at full length it is an exact fingerprint, already handled above.
        const MIN_PREFIX_LEN: i64 = 12;
        let max_prefix_len = Finding::FINGERPRINT_LEN as i64 - 1;
        for (fp,) in db_fetch_all!(
            &self.pool,
            (String,),
            "SELECT fingerprint FROM dismissed_findings
             WHERE length(fingerprint) BETWEEN ? AND ?
               AND COALESCE(repo_full_name, '') = ANY(?)",
            MIN_PREFIX_LEN,
            max_prefix_len,
            &scopes
        )? {
            prefixes_by_len.entry(fp.len()).or_default().insert(fp);
        }

        // Which rules actually fired this review. Recorded so a rule that stopped
        // matching months ago is visible as dead weight instead of quietly
        // suppressing findings nobody has dismissed since.
        let mut hits_by_rule: std::collections::HashMap<&str, i64> =
            std::collections::HashMap::new();

        let kept: Vec<Finding> = findings
            .iter()
            .zip(fingerprints.iter())
            .filter_map(|(f, fp)| {
                if dismissed_set.contains(fp) {
                    return None;
                }
                // Prefix-of-fingerprint only; bidirectional match collided on short strings.
                if prefixes_by_len
                    .iter()
                    .any(|(&len, set)| fp.is_char_boundary(len) && set.contains(&fp[..len]))
                {
                    return None;
                }

                let mut matching = rules_by_detector
                    .get(&f.detector)
                    .into_iter()
                    .flatten()
                    .filter(|rule| {
                        rule.file_pattern
                            .as_deref()
                            .is_none_or(|pat| path_matches_pattern(&f.file, pat))
                            && rule
                                .message_pattern
                                .as_deref()
                                .is_none_or(|pat| f.message.contains(pat))
                    })
                    .peekable();

                // `ignore` wins over every other action, at any scope, and is checked
                // first because it is also the cheapest exit.
                //
                // Precedence deliberately does NOT apply here. `always_warn` rules are
                // auto-approved from *mined PR comments* (see mine::mine_pr_comment_feedback)
                // and are always repo-scoped, so letting the highest-precedence match win
                // outright would let an unauthenticated comment on one PR un-suppress a
                // finding a maintainer had explicitly dismissed org-wide.
                if let Some(rule) = matching.clone().find(|r| r.action == "ignore") {
                    *hits_by_rule.entry(rule.id.as_str()).or_default() += 1;
                    return None;
                }

                // No ignore matched, so at most one severity adjustment applies: the
                // highest-precedence match. Applying *every* match instead let two
                // `downgrade` rules drop one finding two severities, in whatever order
                // the DB happened to return them.
                let mut out = f.clone();
                let applied = matching.peek().copied();
                if let Some(rule) = applied {
                    *hits_by_rule.entry(rule.id.as_str()).or_default() += 1;
                }
                match applied.map(|r| r.action.as_str()) {
                    Some("downgrade") => {
                        out.severity = match out.severity {
                            "blocking" => "warning",
                            "warning" => "info",
                            other => other,
                        };
                    }
                    Some("always_warn") => {
                        if out.severity == "info" {
                            out.severity = "warning";
                        }
                    }
                    // An action we don't recognise is a data problem, not a verdict.
                    // This arm used to `return None`, so one bad row — a typo, a rule
                    // written by a newer build — silently suppressed real findings.
                    Some(_) | None => {}
                }
                Some(out)
            })
            .collect();

        if !hits_by_rule.is_empty() {
            let ids: Vec<String> = hits_by_rule.keys().map(|s| s.to_string()).collect();
            let counts: Vec<i64> = ids.iter().map(|id| hits_by_rule[id.as_str()]).collect();
            // One statement per review, but `match_count` counts *findings* acted on,
            // which is what makes a rule's breadth legible when pruning. Telemetry must
            // never cost a review its findings, so a failure here is logged and dropped.
            if let Err(e) = db_execute!(
                &self.pool,
                "UPDATE learned_rules AS r
                    SET match_count = r.match_count + h.n, last_matched_at = now()
                  FROM UNNEST(?::text[], ?::bigint[]) AS h(id, n)
                  WHERE r.id = h.id",
                &ids,
                &counts
            ) {
                tracing::warn!(error = %e, rules = ids.len(), "recording learned-rule hits failed");
            }
        }

        Ok(kept)
    }
}

#[cfg(test)]
mod scope_tests {
    use super::{path_matches_pattern, scope_chain};

    #[test]
    fn chain_is_global_org_repo() {
        assert_eq!(scope_chain(Some("acme/api")), ["", "acme/", "acme/api"]);
    }

    #[test]
    fn no_repo_is_global_only() {
        assert_eq!(scope_chain(None), [""]);
        assert_eq!(scope_chain(Some("")), [""]);
    }

    #[test]
    fn org_key_cannot_collide_with_a_repo_full_name() {
        // "acme/" is not a legal repo full name, so one org's scope never
        // matches another org's repo rows.
        let acme = scope_chain(Some("acme/api"));
        let other = scope_chain(Some("globex/api"));
        assert!(!acme.contains(&"globex/".to_string()));
        assert!(!other.contains(&"acme/api".to_string()));
    }

    #[test]
    fn file_pattern_is_anchored_and_segment_aware() {
        assert!(path_matches_pattern("src/api/auth.rs", "src/api"));
        assert!(path_matches_pattern("src/api/auth.rs", "src/api/"));
        assert!(path_matches_pattern("src/api/auth.rs", "src/api/auth.rs"));
        // The case plain `contains` got wrong: a *partial* segment. A rule for
        // `src/api` must not reach into a sibling directory sharing its prefix.
        assert!(!path_matches_pattern("src/api-v2/auth.rs", "src/api"));
        assert!(!path_matches_pattern("src/apikeys.rs", "src/api"));
        // Whole-segment containment below the root is kept on purpose: hand-written
        // rules like a bare `migrations` or `node_modules` rely on it.
        assert!(path_matches_pattern("vendor/src/api/auth.rs", "src/api"));
        assert!(path_matches_pattern("db/migrations/001.sql", "migrations"));
        // Hand-written suffix globs.
        assert!(path_matches_pattern(
            "web/ui/types.generated.ts",
            "*.generated.ts"
        ));
        assert!(!path_matches_pattern("web/ui/types.ts", "*.generated.ts"));
    }

    #[test]
    fn scope_chain_is_ordered_by_length() {
        // Both rule queries rank precedence with `length(repo_full_name) DESC` to
        // get repo > org > global without depending on the DB's collation. That is
        // only equivalent to specificity because each scope is strictly longer.
        let chain = scope_chain(Some("acme/api"));
        assert_eq!(chain, vec!["", "acme/", "acme/api"]);
        assert!(chain.windows(2).all(|w| w[0].len() < w[1].len()));
    }
}
