use crate::bot::WebhookPayload;
use crate::detectors::{self, Finding, Findings};
use crate::state::ReviewState;
use anyhow::Result;
use std::sync::LazyLock;
use std::time::Duration;

use std::fmt::Write;

/// Max bytes for a GitHub issue comment body (API limit: 65536).
const MAX_COMMENT_BYTES: usize = 64000;

/// GitHub API max results per page for PR files.
const PER_PAGE: usize = 100;
/// GitHub exposes at most 3,000 PR files (30 pages of 100).
const MAX_PR_FILE_PAGES: usize = 30;
/// Bound reviewer discovery to avoid exhausting an installation's API quota on a large PR.
const MAX_REVIEWER_FILES: usize = 50;
/// Keep review creation within GitHub's inline-comment payload limit.
const MAX_INLINE_COMMENTS: usize = 300;

/// Build a production-configured GitHub API client with timeouts and pooling.
static GITHUB_CLIENT: LazyLock<Option<reqwest::Client>> = LazyLock::new(|| {
    match reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(180))
        .tcp_nodelay(true)
        .build()
    {
        Ok(client) => Some(client),
        Err(e) => {
            eprintln!("Warning: failed to build GitHub API client: {}", e);
            None
        }
    }
});

/// Fetch the complete PR object for a comment-triggered review.
pub async fn fetch_pull_request(
    token: &str,
    repo_name: &str,
    pr_number: i64,
) -> Result<serde_json::Value> {
    let client = GITHUB_CLIENT
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitHub API client not available (failed to initialize)"))?;
    let auth_header = format!("Bearer {token}");
    client
        .get(format!(
            "https://api.github.com/repos/{repo_name}/pulls/{pr_number}"
        ))
        .headers(github_api_headers(&auth_header))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .map_err(Into::into)
}

/// Same auth/User-Agent headers reused across all GitHub API calls.
fn github_api_headers(auth_header: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(auth_header).unwrap(),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static("codasaurus/0.1.0"),
    );
    headers
}

/// Truncate a string to fit within `max_bytes` at a UTF-8 boundary.
fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut idx = max_bytes;
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

/// Fetch all changed files for a PR. GitHub caps this endpoint at 3,000 files.
async fn fetch_pr_files(
    client: &reqwest::Client,
    repo_name: &str,
    pr_number: i64,
    auth_header: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut all_files = Vec::new();
    for page_number in 1..=MAX_PR_FILE_PAGES {
        let resp = client
            .get(format!(
                "https://api.github.com/repos/{}/pulls/{}/files?per_page={}&page={}",
                repo_name, pr_number, PER_PAGE, page_number
            ))
            .headers(github_api_headers(auth_header))
            .send()
            .await?
            .error_for_status()?;
        let page: Vec<serde_json::Value> = resp.json().await?;
        let is_last_page = page.len() < PER_PAGE;
        all_files.extend(page);
        if is_last_page || page_number == MAX_PR_FILE_PAGES {
            return Ok(all_files);
        }
    }

    unreachable!("the bounded page loop always returns")
}

/// Post or update an issue comment using state store for idempotency.
async fn post_or_update_comment(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    body: &str,
    state: &Option<ReviewState>,
) -> Result<i64> {
    let url = format!(
        "https://api.github.com/repos/{}/issues/{}/comments",
        repo_name, pr_number
    );

    if let Some(ref s) = state {
        if let Ok(Some(comment_id)) = s.get_comment_id(repo_name, pr_number) {
            let update_url = format!(
                "https://api.github.com/repos/{}/issues/comments/{}",
                repo_name, comment_id
            );
            let resp = client
                .patch(&update_url)
                .header("Authorization", auth_header)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "codasaurus/0.1.0")
                .json(&serde_json::json!({"body": body}))
                .send()
                .await?;
            if resp.status().is_success() {
                return Ok(comment_id);
            }
            eprintln!(
                "Warning: failed to update comment {} ({}), creating new",
                comment_id,
                resp.status()
            );
        }
    }

    let resp: serde_json::Value = client
        .post(&url)
        .header("Authorization", auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .json(&serde_json::json!({"body": body}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let comment_id = resp["id"].as_i64().unwrap_or(0);
    if comment_id > 0 {
        if let Some(ref s) = state {
            if let Err(e) = s.set_comment_id(repo_name, pr_number, comment_id) {
                eprintln!("Warning: failed to store comment ID: {}", e);
            }
        }
    }

    Ok(comment_id)
}

/// Suggest reviewers based on git history for changed files.
/// Fires parallel API calls (capped via semaphore) to find recent authors.
async fn suggest_reviewers(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    files: &[serde_json::Value],
    pr_author: &str,
) -> Vec<String> {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let author_counts = Arc::new(std::sync::Mutex::new(HashMap::<String, usize>::new()));
    let semaphore = Arc::new(Semaphore::new(10)); // max 10 concurrent requests

    let mut handles = Vec::with_capacity(files.len().min(MAX_REVIEWER_FILES));
    for file in files.iter().take(MAX_REVIEWER_FILES) {
        let filename = match file["filename"].as_str() {
            Some(f) if !f.is_empty() => f.to_string(),
            _ => continue,
        };
        let cl = client.clone();
        let auth = auth_header.to_string();
        let repo = repo_name.to_string();
        let author = pr_author.to_string();
        let counts = Arc::clone(&author_counts);
        let sem = Arc::clone(&semaphore);

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap_or_else(|e| {
                eprintln!("Warning: semaphore closed: {}", e);
                panic!("semaphore closed")
            });

            let commits: Vec<serde_json::Value> = match cl
                .get(format!(
                    "https://api.github.com/repos/{}/commits?path={}&per_page=3",
                    repo, filename
                ))
                .header("Authorization", &auth)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "codasaurus/0.1.0")
                .send()
                .await
            {
                Ok(resp) => match resp.error_for_status() {
                    Ok(response) => response.json().await.unwrap_or_default(),
                    Err(_) => Vec::new(),
                },
                Err(_) => Vec::new(),
            };

            // _permit dropped here → semaphore permit returned automatically

            if !commits.is_empty() {
                let mut local = counts.lock().unwrap();
                for commit in &commits {
                    if let Some(login) = commit["author"]["login"].as_str() {
                        if login != author {
                            *local.entry(login.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }));
    }

    // Wait for all fetches to complete
    for h in handles {
        let _ = h.await;
    }

    let counts = author_counts.lock().unwrap();
    let mut reviewers: Vec<(String, usize)> = counts.clone().into_iter().collect();
    reviewers.sort_by_key(|k| std::cmp::Reverse(k.1));
    reviewers.truncate(5);
    reviewers.into_iter().map(|(name, _)| name).collect()
}

pub async fn review_pr(token: &str, repo_name: &str, payload: &WebhookPayload) -> Result<()> {
    let pr = match &payload.pull_request {
        Some(p) => p,
        None => return Ok(()),
    };

    let pr_number = pr["number"].as_i64().unwrap_or(0);
    let pr_title = pr["title"].as_str().unwrap_or("").to_string();
    let pr_body = pr["body"].as_str().unwrap_or("").to_string();
    let head_sha = pr["head"]["sha"].as_str().unwrap_or("");

    // Incremental: skip if we've already reviewed this commit SHA
    let state = ReviewState::open().ok();
    if let Some(ref s) = state {
        if let Ok(Some(prev_sha)) = s.get_reviewed_sha(repo_name, pr_number) {
            if prev_sha == head_sha {
                return Ok(());
            }
        }
    }

    let config = crate::config::load(None).unwrap_or_default();

    let client = GITHUB_CLIENT
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitHub API client not available (failed to initialize)"))?;
    let auth_header = format!("Bearer {}", token);

    let files = match fetch_pr_files(client, repo_name, pr_number, &auth_header).await {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => return Ok(()),
        Err(e) => {
            eprintln!("Warning: failed to fetch PR files: {}", e);
            return Ok(());
        }
    };

    let mut parsed_files_collected: Vec<crate::parser::ParsedFile> = Vec::new();
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        let patch = file["patch"].as_str().unwrap_or("");
        if !patch.is_empty() && patch.len() < 100_000 {
            let parsed = match crate::parser::parse_unified_diff(filename, patch) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("Warning: failed to parse file {}: {}", filename, e);
                    None
                }
            };
            if let Some(p) = parsed {
                parsed_files_collected.push(p);
            }
        }
    }

    // Run cross-file detectors once. This lets dependency checks see manifest
    // files and prevents repository-level guideline findings from repeating for
    // every changed file.
    let mut findings = if parsed_files_collected.is_empty() {
        Findings::new()
    } else {
        detectors::run_all(&parsed_files_collected, &config)
    };

    // Slop detection — check PR metadata for AI-generation signals
    let pr_author = pr["user"]["login"].as_str().unwrap_or("");
    let commit_messages: Vec<String> = {
        let resp = client
            .get(format!(
                "https://api.github.com/repos/{}/pulls/{}/commits",
                repo_name, pr_number
            ))
            .header("Authorization", &auth_header)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus/0.1.0")
            .send()
            .await;
        match resp {
            Ok(r) => match r.json::<Vec<serde_json::Value>>().await {
                Ok(commits) => commits
                    .iter()
                    .filter_map(|c| c["commit"]["message"].as_str().map(String::from))
                    .collect(),
                Err(_) => vec![],
            },
            Err(_) => vec![],
        }
    };
    let slop_findings = crate::detectors::slop::detect_slop(
        &parsed_files_collected,
        &pr_title,
        &pr_body,
        &commit_messages,
    );
    findings.findings.extend(slop_findings);

    let reviewers = suggest_reviewers(client, &auth_header, repo_name, &files, pr_author).await;

    let mut review_comments: Vec<serde_json::Value> = Vec::new();
    let mut has_blocking = false;
    let mut total_findings = 0;

    for f in &findings.findings {
        total_findings += 1;
        if f.severity == "blocking" {
            has_blocking = true;
        }

        // `parse_unified_diff` maps findings to the new-file line number expected by GitHub.
        if f.line > 0 {
            let comment_body = build_comment_body(f);
            let side = "RIGHT"; // always comment on the new code

            let comment = serde_json::json!({
                "path": f.file,
                "line": f.line,
                "side": side,
                "body": comment_body,
            });
            if review_comments.len() < MAX_INLINE_COMMENTS {
                review_comments.push(comment);
            }
        }
    }

    // Only approve when there are genuinely no findings. Some valid findings are
    // repository-level and have no source line, so an empty inline-comment list
    // must never be treated as a clean review.
    if findings.is_empty() {
        let body = "## 🦕 Codasaurus Review\n\n✅ **No issues found** — auto-approved!\n\n<sub>🦕 Reviewed by [Codasaurus](https://github.com/lohitkolluri/codasaurus)</sub>";
        let review = serde_json::json!({"body": body, "event": "APPROVE"});
        let _: serde_json::Value = client
            .post(format!(
                "https://api.github.com/repos/{}/pulls/{}/reviews",
                repo_name, pr_number
            ))
            .header("Authorization", &auth_header)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus/0.1.0")
            .json(&review)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if !head_sha.is_empty() {
            if let Some(ref s) = state {
                let _ = s.set_reviewed_sha(repo_name, pr_number, head_sha);
            }
        }
        return Ok(());
    }

    let mut body = build_review_body(
        &findings,
        total_findings,
        has_blocking,
        repo_name,
        &pr_title,
        &pr_body,
        &config,
    );

    // Append suggested reviewers
    if !reviewers.is_empty() {
        use std::fmt::Write;
        let _ = writeln!(
            body,
            "\n👥 **Suggested reviewers:** {}",
            reviewers
                .iter()
                .map(|r| format!("@{}", r))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(body);
    }

    // Truncate body to fit within GitHub's 64K comment limit
    body = truncate_utf8(&body, MAX_COMMENT_BYTES).to_string();

    // Try to create a review with inline comments; fall back to single comment
    let review_body = serde_json::json!({
        "body": body,
        "event": if has_blocking { "REQUEST_CHANGES" } else { "COMMENT" },
        "comments": review_comments,
    });

    let resp = client
        .post(format!(
            "https://api.github.com/repos/{}/pulls/{}/reviews",
            repo_name, pr_number
        ))
        .header("Authorization", &auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .json(&review_body)
        .send()
        .await?;

    // If inline review failed (e.g. line numbers don't match), fall back to a single issue comment.
    // Uses the state store to update the previous comment rather than posting a new one.
    if !resp.status().is_success() {
        post_or_update_comment(client, &auth_header, repo_name, pr_number, &body, &state).await?;
    }

    // Record the reviewed commit SHA for incremental review
    if !head_sha.is_empty() {
        if let Some(ref s) = state {
            let _ = s.set_reviewed_sha(repo_name, pr_number, head_sha);
        }
    }

    // Generate and post LLM summary if API key is available
    if let Some(llm_cfg) = crate::llm::LlmConfig::from_env() {
        if let Err(e) = generate_and_post_summary(
            client,
            &auth_header,
            repo_name,
            pr_number,
            &findings,
            &llm_cfg,
            &pr_title,
            &pr_body,
            &state,
        )
        .await
        {
            eprintln!("Warning: failed to generate LLM summary: {}", e);
        }
    }

    Ok(())
}

fn build_comment_body(finding: &Finding) -> String {
    let icon = match finding.severity {
        "blocking" => "🔴",
        "warning" => "🟡",
        _ => "🔵",
    };
    let sev_label = match finding.severity {
        "blocking" => "Blocking",
        "warning" => "Warning",
        _ => "Info",
    };
    use std::fmt::Write;
    let mut body = format!(
        "**{} `{}` — {}**\n\n{}",
        icon, finding.detector, sev_label, finding.message
    );
    if let Some(s) = &finding.suggestion {
        let _ = write!(
            body,
            "\n\n<details><summary>💡 Suggested fix</summary>\n\n> {}\n",
            s
        );
        if let Some(c) = &finding.codemod {
            let _ = write!(body, "\n```suggestion\n{}\n```", c);
        }
        let _ = write!(body, "\n</details>");
    } else if let Some(c) = &finding.codemod {
        let _ = write!(body, "\n\n<details><summary>📝 Committable suggestion</summary>\n\n```suggestion\n{}\n```\n</details>", c);
    }
    body
}

struct CheckResult {
    name: &'static str,
    status: CheckStatus,
    details: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CheckStatus {
    Pass,
    Warning,
    Fail,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "✅ Pass"),
            CheckStatus::Warning => write!(f, "⚠️ Warning"),
            CheckStatus::Fail => write!(f, "❌ Fail"),
        }
    }
}

fn evaluate_pre_merge_checks(
    config: &crate::config::Config,
    pr_title: &str,
    pr_body: &str,
    blocking: usize,
    warnings: usize,
) -> Vec<CheckResult> {
    let mut results = Vec::new();

    if config.pre_merge.require_description {
        let desc = pr_body.trim();
        if desc.is_empty() || desc.len() < 20 {
            results.push(CheckResult {
                name: "PR Description",
                status: CheckStatus::Fail,
                details: "Description is missing or too short".into(),
            });
        } else {
            results.push(CheckResult {
                name: "PR Description",
                status: CheckStatus::Pass,
                details: "Description provided".into(),
            });
        }
    }

    if config.pre_merge.require_title_convention {
        let title = pr_title.trim();
        let conventional = title.contains(':') || title.contains('(');
        if conventional {
            results.push(CheckResult {
                name: "Title Convention",
                status: CheckStatus::Pass,
                details: "Follows type(scope): format".into(),
            });
        } else {
            results.push(CheckResult {
                name: "Title Convention",
                status: CheckStatus::Warning,
                details: "Consider using conventional commit format (type: description)".into(),
            });
        }
    }

    let max_blocking = config.pre_merge.max_blocking;
    if blocking > max_blocking {
        results.push(CheckResult {
            name: "Blocking Issues",
            status: CheckStatus::Fail,
            details: format!(
                "{} blocking issues (max allowed: {})",
                blocking, max_blocking
            ),
        });
    } else if blocking > 0 {
        results.push(CheckResult {
            name: "Blocking Issues",
            status: CheckStatus::Warning,
            details: format!("{} blocking issues found", blocking),
        });
    } else {
        results.push(CheckResult {
            name: "Blocking Issues",
            status: CheckStatus::Pass,
            details: "No blocking issues".into(),
        });
    }

    // Warnings check
    let max_warnings = config.pre_merge.max_warnings;
    if warnings > max_warnings {
        results.push(CheckResult {
            name: "Warnings",
            status: CheckStatus::Warning,
            details: format!("{} warnings (threshold: {})", warnings, max_warnings),
        });
    } else {
        results.push(CheckResult {
            name: "Warnings",
            status: CheckStatus::Pass,
            details: format!("{} warnings", warnings),
        });
    }

    results
}

fn build_review_body(
    findings: &Findings,
    _total: usize,
    has_blocking: bool,
    repo_name: &str,
    pr_title: &str,
    pr_body: &str,
    config: &crate::config::Config,
) -> String {
    use std::collections::BTreeMap;

    let counts = findings.count_by_severity();
    let blocking = counts.get("blocking").copied().unwrap_or(0);
    let warnings = counts.get("warning").copied().unwrap_or(0);
    let infos = counts.get("info").copied().unwrap_or(0);

    let blocking_weight = (blocking * 15) as i32;
    let warning_weight = (warnings * 5) as i32;
    let deduction = blocking_weight + warning_weight + (infos as i32);
    let confidence = (100i32 - deduction.min(95)).max(5) as u32;

    let score_emoji = match confidence {
        90..=100 => "🟢",
        70..=89 => "🟡",
        _ => "🔴",
    };

    let verdict = if has_blocking {
        "⛔ Changes requested"
    } else if warnings > 0 {
        "⚠️ Review with suggestions"
    } else {
        "ℹ️ Info only"
    };

    let mut body = String::with_capacity(2048);
    let _ = writeln!(body, "## 🦕 Codasaurus Review");
    let _ = writeln!(body);
    let _ = writeln!(
        body,
        "**{}** | 🔴 **{} blocking** | 🟡 **{} warnings** | 🔵 **{} info**",
        verdict, blocking, warnings, infos
    );
    let _ = writeln!(
        body,
        "**Review confidence:** {} {}%",
        score_emoji, confidence
    );
    let _ = writeln!(body);

    // Single pass: group findings by file AND build severity buckets
    let mut by_file: BTreeMap<String, Vec<&Finding>> = BTreeMap::new();
    let mut blocking_findings: Vec<&Finding> = Vec::new();
    let mut warning_findings: Vec<&Finding> = Vec::new();
    let mut info_findings: Vec<&Finding> = Vec::new();
    for f in &findings.findings {
        by_file.entry(f.file.clone()).or_default().push(f);
        match f.severity {
            "blocking" => blocking_findings.push(f),
            "warning" => warning_findings.push(f),
            _ => info_findings.push(f),
        }
    }

    // Per-file finding tables
    for (file_path, file_findings) in &by_file {
        let _ = writeln!(body, "### `{}`", file_path);
        let _ = writeln!(body);
        let _ = writeln!(body, "| Line | Severity | Finding |");
        let _ = writeln!(body, "| --- | --- | --- |");
        for f in file_findings {
            let icon = match f.severity {
                "blocking" => "🔴",
                "warning" => "🟡",
                _ => "🔵",
            };
            let line_str = if f.line > 0 {
                format!(":{}", f.line)
            } else {
                String::new()
            };
            let _ = writeln!(
                body,
                "| `{}` | {} {} | `{}` — {} |",
                line_str.trim_start_matches(':'),
                icon,
                f.severity,
                f.detector,
                f.message
            );
        }
        let _ = writeln!(body);
    }

    // Collapsible detailed breakdown by severity — uses pre-bucketed Vecs (0 additional iterations)
    let _ = writeln!(body, "---");
    let _ = writeln!(body);

    if !blocking_findings.is_empty() {
        let _ = writeln!(
            body,
            "<details><summary>🔴 **{} Blocking**</summary>",
            blocking
        );
        let _ = writeln!(body);
        for f in &blocking_findings {
            let _ = writeln!(body, "- **`{}:{}`** — {}", f.file, f.line, f.message);
            if let Some(ref s) = f.suggestion {
                let _ = writeln!(body, "  - > 💡 {}", s);
            }
        }
        let _ = writeln!(body, "</details>");
        let _ = writeln!(body);
    }

    if !warning_findings.is_empty() {
        let _ = writeln!(
            body,
            "<details><summary>🟡 **{} Warnings**</summary>",
            warnings
        );
        let _ = writeln!(body);
        for f in &warning_findings {
            let _ = writeln!(body, "- **`{}:{}`** — {}", f.file, f.line, f.message);
            if let Some(ref s) = f.suggestion {
                let _ = writeln!(body, "  - > 💡 {}", s);
            }
        }
        let _ = writeln!(body, "</details>");
        let _ = writeln!(body);
    }

    if !info_findings.is_empty() {
        let _ = writeln!(body, "<details><summary>🔵 **{} Info**</summary>", infos);
        let _ = writeln!(body);
        for f in &info_findings {
            let _ = writeln!(body, "- **`{}:{}`** — {}", f.file, f.line, f.message);
        }
        let _ = writeln!(body, "</details>");
        let _ = writeln!(body);
    }

    // Pre-merge checks
    let checks = evaluate_pre_merge_checks(config, pr_title, pr_body, blocking, warnings);
    if !checks.is_empty() {
        let _ = writeln!(body, "🚥 **Pre-merge Checks**");
        let _ = writeln!(body);
        let _ = writeln!(body, "| Check | Status | Details |");
        let _ = writeln!(body, "| --- | --- | --- |");
        for check in &checks {
            let _ = writeln!(
                body,
                "| {} | {} | {} |",
                check.name, check.status, check.details
            );
        }
        let _ = writeln!(body);
    }

    let _ = writeln!(body, "---");
    let _ = writeln!(
        body,
        "<sub>🦕 Reviewed by [Codasaurus](https://github.com/lohitkolluri/codasaurus) — `{}`</sub>",
        repo_name
    );
    let _ = writeln!(body);

    body
}

#[allow(clippy::too_many_arguments)]
async fn generate_and_post_summary(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    findings: &Findings,
    llm_cfg: &crate::llm::LlmConfig,
    pr_title: &str,
    pr_body: &str,
    state: &Option<ReviewState>,
) -> Result<()> {
    let mut findings_text = String::new();
    for f in &findings.findings {
        let _ = writeln!(
            findings_text,
            "- {}: {} (line {})",
            f.severity, f.message, f.line
        );
    }

    let prompt = format!(
        r#"Generate a concise PR review summary (2-3 paragraphs) for the following code review results.

PR Title: {}
PR Description: {}

Findings:
{}

Write a helpful summary that:
1. Gives an overall assessment
2. Highlights the most critical issues
3. Provides actionable advice
Keep it under 200 words and professional in tone."#,
        pr_title, pr_body, findings_text
    );

    let output = crate::llm::review_diff(&prompt, llm_cfg, None).await?;

    let summary_body = format!(
        "## 📋 AI Review Summary\n\n{}\n\n---\n_Generated by Codasaurus LLM review_",
        output.summary.as_deref().unwrap_or(&output.verdict)
    );

    // Use state store for comment editing to prevent duplicates
    post_or_update_comment(
        client,
        auth_header,
        repo_name,
        pr_number,
        &summary_body,
        state,
    )
    .await?;

    Ok(())
}
