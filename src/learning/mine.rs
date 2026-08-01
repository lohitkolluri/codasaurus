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
    "wontfix",
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

/// After a dashboard/comment dismissal, promote repeated detector+file noise into a rule.
pub async fn promote_dismissal_to_rule(
    store: &LearningStore,
    detector: &str,
    file: &str,
    message: &str,
) -> Result<()> {
    if detector.is_empty() || detector == "manual" {
        return Ok(());
    }
    let count = store.count_dismissals_for_detector(detector).await?;
    // After 3 dismissals of the same detector, learn an ignore for that file path stem.
    if count < 3 {
        return Ok(());
    }
    let file_stem = file_pattern_from_path(file);
    let msg_pat = message_pattern_hint(message);
    let rule = LearnedRule {
        id: format!("auto-{}", Uuid::new_v4()),
        detector: detector.to_string(),
        file_pattern: file_stem,
        message_pattern: msg_pat,
        action: RuleAction::Ignore,
        reason: format!("auto-learned after {count} dismissals of `{detector}`"),
        created_at: String::new(),
    };
    store.add_rule_async(&rule).await?;
    Ok(())
}

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
                let rule = LearnedRule {
                    id: format!("fp-{}", Uuid::new_v4()),
                    detector: det,
                    file_pattern: None,
                    message_pattern: None,
                    action: RuleAction::Ignore,
                    reason: format!("mined false-positive signal on PR #{pr_number}"),
                    created_at: String::new(),
                };
                store.add_rule_async(&rule).await?;
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
                    created_at: String::new(),
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
}
