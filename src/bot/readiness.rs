//! Merge-readiness score (0-5): hard blockers zero it, weighted soft signals
//! calibrate it. Fully deterministic and explainable (MergeGuard pattern).

use crate::config::ReadinessConfig;
use crate::detectors::Finding;
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};

pub struct ReadinessSignal {
    pub name: &'static str,
    pub weight: f64,
    pub passed: bool,
    pub note: String,
}

pub struct ReadinessReport {
    pub score: u8,
    pub blockers: Vec<String>,
    pub signals: Vec<ReadinessSignal>,
}

impl ReadinessReport {
    pub fn markdown(&self) -> String {
        let mut md = format!("**Merge readiness: {}/5**\n\n", self.score);
        if !self.blockers.is_empty() {
            md.push_str("**Hard blockers:**\n");
            for b in &self.blockers {
                md.push_str(&format!("- ❌ {b}\n"));
            }
            md.push('\n');
        }
        md.push_str("| Signal | Weight | Result |\n|---|---|---|\n");
        for s in &self.signals {
            let mark = if s.passed { "✅" } else { "⚠️" };
            md.push_str(&format!(
                "| {} | {:.1} | {} {} |\n",
                s.name, s.weight, mark, s.note
            ));
        }
        md
    }
}

fn signal(name: &'static str, weight: f64, passed: bool, note: String) -> ReadinessSignal {
    ReadinessSignal {
        name,
        weight,
        passed,
        note,
    }
}

/// Score a PR: blockers (gate, conflicts, CI, approvals, reachable critical
/// vulns) zero the score; otherwise soft signals are weight-averaged to 0-5.
#[allow(clippy::too_many_arguments)]
pub async fn evaluate(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    repo: &str,
    pr_number: i64,
    head_sha: &str,
    pr: &serde_json::Value,
    findings: &[Finding],
    gate_passed: bool,
    cfg: &ReadinessConfig,
) -> ReadinessReport {
    let mergeable = pr["mergeable"].as_bool();
    let additions = pr["additions"].as_u64().unwrap_or(0);
    let deletions = pr["deletions"].as_u64().unwrap_or(0);
    let updated_at = pr["updated_at"].as_str().unwrap_or("");

    let (failing_ci, approvals, changes_requested) =
        fetch_github_state(client, headers, repo, pr_number, head_sha).await;

    let mut blockers = Vec::new();
    if cfg.block_on_blockers {
        if !gate_passed {
            blockers.push("quality gate failed".into());
        }
        if mergeable == Some(false) {
            blockers.push("merge conflict".into());
        }
        if failing_ci {
            blockers.push("failing CI check run".into());
        }
        if approvals < cfg.require_approvals {
            blockers.push(format!(
                "missing {} required approval(s), got {approvals}",
                cfg.require_approvals
            ));
        }
        if has_reachable_critical(findings) {
            blockers.push("reachable critical/high OSV vulnerability".into());
        }
    }

    let mut signals = Vec::new();
    signals.push(signal(
        "Review threads resolved",
        2.0,
        !changes_requested,
        if changes_requested {
            "changes requested".into()
        } else {
            "no changes requested".into()
        },
    ));
    let open_non_info = findings.iter().filter(|f| f.severity != "info").count();
    signals.push(signal(
        "Open findings",
        2.5,
        open_non_info <= 2,
        format!("{open_non_info} warning/blocking finding(s)"),
    ));
    let size = additions + deletions;
    signals.push(signal(
        "PR size",
        1.5,
        size < 500,
        format!("{size} changed lines"),
    ));
    let days_old = stale_days(updated_at);
    signals.push(signal(
        "Staleness",
        1.0,
        days_old < 7,
        format!("{days_old}d since update"),
    ));
    signals.push(signal(
        "Approvals",
        1.0,
        approvals >= cfg.require_approvals,
        format!("{approvals} approval(s)"),
    ));

    let total: f64 = signals.iter().map(|s| s.weight).sum();
    let earned: f64 = signals.iter().filter(|s| s.passed).map(|s| s.weight).sum();
    let score = if blockers.is_empty() && total > 0.0 {
        (5.0 * earned / total).round() as u8
    } else {
        0
    };

    ReadinessReport {
        score,
        blockers,
        signals,
    }
}

fn has_reachable_critical(findings: &[Finding]) -> bool {
    findings.iter().any(|f| {
        f.detector == "vulnerabilities"
            && f.reachability.as_deref() == Some("reachable")
            && f.severity == "warning"
    })
}

fn stale_days(updated_at: &str) -> u64 {
    let Ok(ts) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return 0;
    };
    chrono::Utc::now()
        .signed_duration_since(ts.with_timezone(&chrono::Utc))
        .num_days()
        .max(0) as u64
}

/// Best-effort GitHub state: failing CI checks (excluding our own Codasaurus
/// run), distinct approvals, and whether changes were requested.
async fn fetch_github_state(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    repo: &str,
    pr_number: i64,
    head_sha: &str,
) -> (bool, usize, bool) {
    let mut failing_ci = false;
    if !head_sha.is_empty() {
        let url = format!(
            "https://api.github.com/repos/{repo}/commits/{head_sha}/check-runs?per_page=20"
        );
        if let Ok(resp) = retry_async(
            &RetryConfig::quick(),
            "readiness_check_runs",
            &is_reqwest_error_retryable,
            || async {
                client
                    .get(&url)
                    .headers(headers.clone())
                    .header("User-Agent", crate::bot::USER_AGENT)
                    .header("Accept", "application/vnd.github+json")
                    .send()
                    .await
                    .map_err(Into::into)
            },
        )
        .await
        {
            if resp.status().is_success() {
                if let Ok(v) = resp.json::<serde_json::Value>().await {
                    failing_ci = v["check_runs"]
                        .as_array()
                        .map(|runs| {
                            runs.iter().any(|r| {
                                r["name"].as_str() != Some("Codasaurus")
                                    && matches!(
                                        r["conclusion"].as_str(),
                                        Some(
                                            "failure"
                                                | "action_required"
                                                | "cancelled"
                                                | "timed_out"
                                        )
                                    )
                            })
                        })
                        .unwrap_or(false);
                }
            }
        }
    }

    let mut approvals = 0usize;
    let mut changes_requested = false;
    let url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}/reviews?per_page=100");
    if let Ok(resp) = retry_async(
        &RetryConfig::quick(),
        "readiness_reviews",
        &is_reqwest_error_retryable,
        || async {
            client
                .get(&url)
                .headers(headers.clone())
                .header("User-Agent", crate::bot::USER_AGENT)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await
    {
        if resp.status().is_success() {
            if let Ok(reviews) = resp.json::<Vec<serde_json::Value>>().await {
                let mut approvers = std::collections::HashSet::new();
                for r in reviews {
                    match r["state"].as_str() {
                        Some("APPROVED") => {
                            if let Some(login) = r["user"]["login"].as_str() {
                                approvers.insert(login.to_string());
                            }
                        }
                        Some("CHANGES_REQUESTED") => changes_requested = true,
                        _ => {}
                    }
                }
                approvals = approvers.len();
            }
        }
    }

    (failing_ci, approvals, changes_requested)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(detector: &str, severity: &'static str, reachability: Option<&str>) -> Finding {
        Finding {
            detector: detector.to_string(),
            severity,
            file: "a.rs".into(),
            line: 1,
            column: 0,
            message: "m".into(),
            suggestion: None,
            evidence: None,
            codemod: None,
            confidence: None,
            judge_rationale: None,
            reachability: reachability.map(str::to_string),
        }
    }

    fn pr(mergeable: Option<bool>, updated_at: &str) -> serde_json::Value {
        serde_json::json!({
            "mergeable": mergeable,
            "additions": 20,
            "deletions": 10,
            "updated_at": updated_at,
        })
    }

    #[test]
    fn reachable_critical_vuln_is_a_blocker() {
        assert!(has_reachable_critical(&[finding(
            "vulnerabilities",
            "warning",
            Some("reachable")
        )]));
        assert!(!has_reachable_critical(&[finding(
            "vulnerabilities",
            "warning",
            Some("manifest_only")
        )]));
        assert!(!has_reachable_critical(&[finding(
            "secrets", "warning", None
        )]));
    }

    #[test]
    fn blockers_zero_the_score() {
        let cfg = ReadinessConfig::default();
        let report = evaluate_sync(&cfg, pr(Some(false), "2026-08-01T00:00:00Z"), &[], false);
        assert_eq!(report.score, 0);
        assert!(!report.blockers.is_empty());
    }

    #[test]
    fn clean_pr_scores_high() {
        let cfg = ReadinessConfig {
            enabled: true,
            require_approvals: 0,
            block_on_blockers: true,
        };
        let report = evaluate_sync(&cfg, pr(Some(true), "2026-08-02T00:00:00Z"), &[], true);
        assert!(report.score >= 4, "{:?}", report.score);
        assert!(report.blockers.is_empty());
    }

    #[test]
    fn missing_approvals_blocks_when_required() {
        let cfg = ReadinessConfig {
            enabled: true,
            require_approvals: 2,
            block_on_blockers: true,
        };
        let report = evaluate_sync(&cfg, pr(Some(true), "2026-08-02T00:00:00Z"), &[], true);
        assert_eq!(report.score, 0);
        assert!(
            report.blockers.iter().any(|b| b.contains("approval")),
            "{:?}",
            report.blockers
        );
    }

    /// Sync wrapper: GitHub fetches fail fast in tests (no client), so only
    /// pure-GitHub-independent facts (mergeable, gate, findings) are exercised.
    fn evaluate_sync(
        cfg: &ReadinessConfig,
        pr: serde_json::Value,
        findings: &[Finding],
        gate_passed: bool,
    ) -> ReadinessReport {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(evaluate(
            &reqwest::Client::new(),
            &reqwest::header::HeaderMap::new(),
            "acme/repo",
            1,
            "",
            &pr,
            findings,
            gate_passed,
            cfg,
        ))
    }
}
