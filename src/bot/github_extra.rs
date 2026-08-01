//! Extra GitHub App actions: request reviewers + Check Runs.

use crate::detectors::Finding;
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use reqwest::header::HeaderMap;

const USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));
/// GitHub caps check-run annotations at 50.
const MAX_ANNOTATIONS: usize = 50;

/// Split CODEOWNERS owners into user logins vs `org/team` slugs.
pub fn split_reviewers(owners: &[String]) -> (Vec<String>, Vec<String>) {
    let mut users = Vec::new();
    let mut teams = Vec::new();
    for o in owners {
        let o = o.trim().trim_start_matches('@');
        if o.is_empty() {
            continue;
        }
        if o.contains('/') {
            // API wants the team slug only (last segment).
            let slug = o.rsplit('/').next().unwrap_or(o).to_string();
            if !teams.iter().any(|t| t == &slug) {
                teams.push(slug);
            }
        } else if !users.iter().any(|u| u == o) {
            users.push(o.to_string());
        }
    }
    (users, teams)
}

/// Request PR reviewers (users + teams). Best-effort; ignores permission errors.
pub async fn request_pull_reviewers(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    pr_number: i64,
    owners: &[String],
    pr_author: &str,
) -> Result<()> {
    let (mut users, teams) = split_reviewers(owners);
    users.retain(|u| !u.eq_ignore_ascii_case(pr_author));
    users.truncate(8);
    if users.is_empty() && teams.is_empty() {
        return Ok(());
    }

    let url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}/requested_reviewers");
    let body = serde_json::json!({
        "reviewers": users,
        "team_reviewers": teams,
    });

    let resp = retry_async(
        &RetryConfig::api_default(),
        "request_reviewers",
        &is_reqwest_error_retryable,
        || async {
            client
                .post(&url)
                .headers(headers.clone())
                .header("User-Agent", USER_AGENT)
                .json(&body)
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        tracing::warn!(%status, body = %text.chars().take(200).collect::<String>(), "request_reviewers failed");
    }
    Ok(())
}

fn annotation_level(severity: &str) -> &'static str {
    match severity {
        "blocking" => "failure",
        "warning" => "warning",
        _ => "notice",
    }
}

/// Create a completed Check Run with annotations mirroring findings.
pub async fn create_findings_check_run(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    head_sha: &str,
    findings: &[Finding],
    has_blocking: bool,
) -> Result<()> {
    if head_sha.is_empty() {
        return Ok(());
    }

    let mut annotations = Vec::new();
    for f in findings.iter().filter(|f| f.line > 0).take(MAX_ANNOTATIONS) {
        let msg: String = f.message.chars().take(2000).collect();
        annotations.push(serde_json::json!({
            "path": f.file,
            "start_line": f.line,
            "end_line": f.line,
            "annotation_level": annotation_level(f.severity),
            "message": msg,
            "title": f.detector,
        }));
    }

    let blocking = findings.iter().filter(|f| f.severity == "blocking").count();
    let warning = findings.iter().filter(|f| f.severity == "warning").count();
    let info = findings.iter().filter(|f| f.severity == "info").count();
    let conclusion = if has_blocking || blocking > 0 {
        "action_required"
    } else if warning > 0 {
        "neutral"
    } else {
        "success"
    };

    let summary = format!(
        "Codasaurus found **{blocking}** blocking, **{warning}** warning, **{info}** info.\n\n\
         Inline review comments and walkthrough are on the PR."
    );

    let url = format!("https://api.github.com/repos/{repo}/check-runs");
    let body = serde_json::json!({
        "name": "Codasaurus",
        "head_sha": head_sha,
        "status": "completed",
        "conclusion": conclusion,
        "output": {
            "title": if blocking > 0 {
                format!("{blocking} blocking issue(s)")
            } else if warning > 0 {
                format!("{warning} warning(s)")
            } else {
                "No blocking issues".into()
            },
            "summary": summary,
            "annotations": annotations,
        },
    });

    let resp = retry_async(
        &RetryConfig::api_default(),
        "create_check_run",
        &is_reqwest_error_retryable,
        || async {
            client
                .post(&url)
                .headers(headers.clone())
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/vnd.github+json")
                .json(&body)
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        tracing::warn!(%status, body = %text.chars().take(240).collect::<String>(), "create_check_run failed (need Checks: write?)");
    }
    Ok(())
}

/// Apply suggested labels to a PR (issues API).
pub async fn apply_labels(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    pr_number: i64,
    labels: &[String],
) -> Result<()> {
    if labels.is_empty() {
        return Ok(());
    }
    let url = format!("https://api.github.com/repos/{repo}/issues/{pr_number}/labels");
    let body = serde_json::json!({ "labels": labels });
    let resp = retry_async(
        &RetryConfig::api_default(),
        "apply_labels",
        &is_reqwest_error_retryable,
        || async {
            client
                .post(&url)
                .headers(headers.clone())
                .header("User-Agent", USER_AGENT)
                .json(&body)
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await?;
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "apply_labels failed");
    }
    Ok(())
}

/// Suggest labels from changed paths + finding detectors.
pub fn suggest_labels(paths: &[String], detectors: &[String]) -> Vec<String> {
    let mut labels = Vec::new();
    let joined = paths.join("\n").to_lowercase();
    if paths.iter().any(|p| p.contains("test") || p.ends_with("_test.rs") || p.contains("spec.")) {
        labels.push("tests".into());
    }
    if joined.contains("dockerfile") || joined.contains(".github/workflows") {
        labels.push("ci".into());
    }
    if paths.iter().any(|p| {
        p.ends_with(".md") || p.contains("docs/") || p.eq_ignore_ascii_case("readme.md")
    }) {
        labels.push("documentation".into());
    }
    if detectors.iter().any(|d| d == "secrets" || d == "vulnerabilities") {
        labels.push("security".into());
    }
    if detectors
        .iter()
        .any(|d| d == "hallucinated-imports" || d == "phantom-deps")
    {
        labels.push("dependencies".into());
    }
    if labels.is_empty() {
        labels.push("needs-review".into());
    }
    labels.sort();
    labels.dedup();
    labels.truncate(5);
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_users_and_teams() {
        let (u, t) = split_reviewers(&["alice".into(), "acme/backend".into(), "@bob".into()]);
        assert!(u.contains(&"alice".into()));
        assert!(u.contains(&"bob".into()));
        assert_eq!(t, vec!["backend".to_string()]);
    }

    #[test]
    fn suggests_security_label() {
        let labels = suggest_labels(
            &["src/auth.rs".into()],
            &["secrets".into()],
        );
        assert!(labels.contains(&"security".into()));
    }
}
