//! Mine human PR feedback into learned rules (LGTM / pushback / false-positive).

use crate::learning::store::LearningStore;
use crate::learning::{LearnedRule, RuleAction};
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use reqwest::header::HeaderMap;
use uuid::Uuid;

const USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));

/// Patterns that suggest a finding was a false positive / should be ignored.
const FALSE_POSITIVE_HINTS: &[&str] = &[
    "false positive",
    "false-positive",
    "not a bug",
    "not an issue",
    "ignore this",
    "won't fix",
    "wont fix",
    "noise",
    "not relevant",
];

/// Patterns that suggest the team wants the class of finding kept (always warn).
const PUSHBACK_HINTS: &[&str] = &[
    "please fix",
    "must fix",
    "blocking",
    "do not merge",
    "request changes",
    "needs fix",
];

/// Patterns that suggest overall approval (used for telemetry / soft rules only).
const LGTM_HINTS: &[&str] = &["lgtm", "looks good", "ship it", "approved"];

/// Security-class detectors never auto-promote from non-maintainer signals alone.
fn is_security_detector(detector: &str) -> bool {
    matches!(
        detector,
        "secrets" | "vulnerabilities" | "iac" | "risky-patterns" | "risky_patterns"
    )
}

/// After a dashboard/comment dismissal, promote repeated detector noise into a rule.
///
/// Poisoning guard:
/// - Security detectors require at least one maintainer dismissal.
/// - Other detectors require a maintainer dismissal, or dismissals across
///   [`MIN_DISTINCT_PRS`] distinct PRs **within the same repo**.
/// - Learned rules are always scoped to `repo_full_name` when provided.
pub async fn promote_dismissal_to_rule(
    store: &LearningStore,
    detector: &str,
    file: &str,
    message: &str,
    repo_full_name: Option<&str>,
) -> Result<()> {
    if detector.is_empty() || detector == "manual" || detector == "reaction" {
        return Ok(());
    }
    let maintainer_hit = store
        .count_maintainer_dismissals_for_detector(detector, repo_full_name)
        .await?;
    let distinct_prs = store
        .count_distinct_prs_for_detector(detector, repo_full_name)
        .await?;

    let allow = if is_security_detector(detector) {
        maintainer_hit >= 1
    } else {
        maintainer_hit >= 1 || distinct_prs >= MIN_DISTINCT_PRS
    };
    if !allow {
        return Ok(());
    }

    let file_stem = file_pattern_from_path(file);
    let msg_pat = message_pattern_hint(message);
    let reason = if maintainer_hit >= 1 {
        format!("auto-learned after maintainer dismiss of `{detector}`")
    } else {
        format!("auto-learned after dismissals of `{detector}` across {distinct_prs} PRs")
    };
    let rule = LearnedRule {
        id: format!("auto-{}", Uuid::new_v4()),
        detector: detector.to_string(),
        file_pattern: file_stem,
        message_pattern: msg_pat,
        action: RuleAction::Ignore,
        reason,
        created_at: chrono::Utc::now(),
        repo_full_name: repo_full_name.filter(|r| !r.is_empty()).map(str::to_string),
    };
    store.add_rule_async(&rule).await?;
    Ok(())
}

/// Distinct PRs required before a non-maintainer dismissal stream can auto-learn
/// (non-security detectors only, and always repo-scoped).
pub const MIN_DISTINCT_PRS: i64 = 3;

fn file_pattern_from_path(file: &str) -> Option<String> {
    if file.is_empty() || file == "unknown" {
        return None;
    }
    // Prefer directory prefix for broader learning.
    if let Some((dir, _)) = file.rsplit_once('/') {
        if !dir.is_empty() {
            return Some(dir.to_string());
        }
    }
    Some(file.to_string())
}

fn message_pattern_hint(message: &str) -> Option<String> {
    let m = message.trim();
    if m.len() < 12 {
        return None;
    }
    // Take a stable-ish substring (first ~40 chars of alphanumeric words).
    let cleaned: String = m
        .chars()
        .take(48)
        .collect::<String>()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.len() >= 8 {
        Some(cleaned)
    } else {
        None
    }
}

/// Scan recent issue + review comments on a PR for false-positive / pushback signals.
pub async fn mine_pr_comment_feedback(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    pr_number: i64,
    store: &LearningStore,
) -> Result<usize> {
    let mut learned = 0usize;

    let issue_url =
        format!("https://api.github.com/repos/{repo}/issues/{pr_number}/comments?per_page=40");
    let review_url =
        format!("https://api.github.com/repos/{repo}/pulls/{pr_number}/comments?per_page=40");

    let (issue_comments, review_comments) = tokio::join!(
        fetch_comment_bodies(client, headers, &issue_url),
        fetch_comment_bodies(client, headers, &review_url),
    );

    let mut all = Vec::new();
    all.extend(issue_comments.unwrap_or_default());
    all.extend(review_comments.unwrap_or_default());

    for body in all {
        let lower = body.to_ascii_lowercase();
        // Skip our own command traffic.
        if lower.contains("@codasaurus") && (lower.contains("help") || lower.contains("review")) {
            continue;
        }

        let detector = extract_detector_mention(&lower);

        if FALSE_POSITIVE_HINTS.iter().any(|h| lower.contains(h)) {
            if let Some(det) = detector {
                // Record as a dismissal signal; promote only via the poisoning guard.
                // Never auto-promote security detectors from mined (unauthenticated) comments.
                if is_security_detector(&det) {
                    continue;
                }
                let fp = format!("mined-fp:{pr_number}:{det}");
                store
                    .dismiss_fingerprint_for_repo(
                        &fp,
                        &det,
                        "",
                        "mined false-positive signal",
                        Some(repo),
                        Some(pr_number),
                        None,
                        false,
                    )
                    .await?;
                learned += 1;
            }
        } else if PUSHBACK_HINTS.iter().any(|h| lower.contains(h)) {
            if let Some(det) = detector {
                let rule = LearnedRule {
                    id: format!("pb-{}", Uuid::new_v4()),
                    detector: det,
                    file_pattern: None,
                    message_pattern: None,
                    action: RuleAction::AlwaysWarn,
                    reason: format!("mined pushback signal on PR #{pr_number}"),
                    created_at: chrono::Utc::now(),
                    repo_full_name: Some(repo.to_string()),
                };
                store.add_rule_async(&rule).await?;
                learned += 1;
            }
        } else if LGTM_HINTS.iter().any(|h| lower.contains(h)) {
            // Soft: no rule — approval doesn't suppress detectors.
            let _ = lower;
        }
    }

    Ok(learned)
}

async fn fetch_comment_bodies(
    client: &reqwest::Client,
    headers: &HeaderMap,
    url: &str,
) -> Result<Vec<String>> {
    let resp = retry_async(
        &RetryConfig::quick(),
        "fetch_pr_comments",
        &is_reqwest_error_retryable,
        || async {
            client
                .get(url)
                .headers(headers.clone())
                .header("User-Agent", USER_AGENT)
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    let values: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
    Ok(values
        .into_iter()
        .filter_map(|v| v.get("body")?.as_str().map(str::to_string))
        .collect())
}

fn extract_detector_mention(lower: &str) -> Option<String> {
    const DETECTORS: &[&str] = &[
        "secrets",
        "vulnerabilities",
        "hallucinated-imports",
        "phantom-deps",
        "todo-leaks",
        "over-engineering",
        "boilerplate",
        "stale-api",
        "guidelines",
        "graph",
        "slop",
        "policy",
    ];
    for d in DETECTORS {
        if lower.contains(d) {
            return Some((*d).to_string());
        }
    }
    // `fingerprint: abc` near false-positive → treat as generic ignore via dismissal path only.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_pattern_uses_dir() {
        assert_eq!(
            file_pattern_from_path("src/api/auth.rs").as_deref(),
            Some("src/api")
        );
    }

    #[test]
    fn extracts_detector() {
        assert_eq!(
            extract_detector_mention("this secrets finding is a false positive"),
            Some("secrets".into())
        );
    }

    #[test]
    fn security_detectors_flagged() {
        assert!(is_security_detector("secrets"));
        assert!(is_security_detector("vulnerabilities"));
        assert!(!is_security_detector("boilerplate"));
    }
}
