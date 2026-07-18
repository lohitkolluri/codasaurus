use crate::bot::WebhookPayload;
use crate::detectors::{self, Finding, Findings};
use crate::state::ReviewState;
use anyhow::Result;

/// Post or update an issue comment using state store for idempotency.
/// If a stored comment_id exists, PATCH it; otherwise POST and store the new ID.
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
/// Calls GitHub API to find recent authors for each file.
async fn suggest_reviewers(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    files: &[serde_json::Value],
    pr_author: &str,
) -> Vec<String> {
    use std::collections::HashMap;
    let mut author_counts: HashMap<String, usize> = HashMap::new();

    for file in files {
        let filename = file["filename"].as_str().unwrap_or("");
        if filename.is_empty() {
            continue;
        }

        if let Ok(resp) = client
            .get(format!(
                "https://api.github.com/repos/{}/commits?path={}&per_page=3",
                repo_name, filename
            ))
            .header("Authorization", auth_header)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus/0.1.0")
            .send()
            .await
        {
            if let Ok(commits) = resp.json::<Vec<serde_json::Value>>().await {
                for commit in commits {
                    if let Some(author) = commit["author"]["login"].as_str() {
                        if author != pr_author {
                            *author_counts.entry(author.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
    }

    let mut reviewers: Vec<(String, usize)> = author_counts.into_iter().collect();
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

    let client = reqwest::Client::new();
    let auth_header = format!("Bearer {}", token);

    let files_text: String = client
        .get(format!(
            "https://api.github.com/repos/{}/pulls/{}/files",
            repo_name, pr_number
        ))
        .header("Authorization", &auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .send()
        .await?
        .text()
        .await?;

    let files: Vec<serde_json::Value> = match serde_json::from_str(&files_text) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: failed to parse PR files response: {}", e);
            return Ok(());
        }
    };
    if files.is_empty() {
        return Ok(());
    }

    let mut findings = Findings::new();
    let mut parsed_files_collected: Vec<crate::parser::ParsedFile> = Vec::new();
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        let patch = file["patch"].as_str().unwrap_or("");
        if !patch.is_empty() && patch.len() < 100_000 {
            let parsed = match crate::parser::parse_file(filename, patch) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("Warning: failed to parse file {}: {}", filename, e);
                    None
                }
            };
            if let Some(p) = parsed {
                findings
                    .extend(detectors::run_all(std::slice::from_ref(&p), &crate::config::Config::default()).findings);
                parsed_files_collected.push(p);
            }
        }
    }

    // Filter findings through the learning store (user dismissals)
    if let Ok(store) = crate::learning::store::LearningStore::open() {
        if let Ok(filtered) = store.filter_findings(&findings.findings) {
            findings.findings = filtered;
        }
    }

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

    let reviewers = suggest_reviewers(&client, &auth_header, repo_name, &files, pr_author).await;

    let mut review_comments: Vec<serde_json::Value> = Vec::new();
    let mut has_blocking = false;
    let mut total_findings = 0;

    for f in &findings.findings {
        total_findings += 1;
        if f.severity == "blocking" {
            has_blocking = true;
        }

        // Map finding line number to the PR diff position
        if f.line > 0 {
            let comment_body = build_comment_body(f);
            let side = "RIGHT"; // always comment on the new code

            let comment = serde_json::json!({
                "path": f.file,
                "line": f.line,
                "side": side,
                "body": comment_body,
            });
            review_comments.push(comment);
        }
    }

    // If no findings, auto-approve via the PR reviews API
    if review_comments.is_empty() {
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
        post_or_update_comment(&client, &auth_header, repo_name, pr_number, &body, &state).await?;
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
            &client,
            &auth_header,
            repo_name,
            pr_number,
            &findings,
            &llm_cfg,
            &pr_title,
            &pr_body,
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
        let _ = write!(body, "\n\n<details><summary>💡 Suggested fix</summary>\n\n> {}\n", s);
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
            details: format!("{} blocking issues (max allowed: {})", blocking, max_blocking),
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
    use std::fmt::Write;

    let counts = findings.count_by_severity();
    let blocking = counts.get("blocking").copied().unwrap_or(0);
    let warnings = counts.get("warning").copied().unwrap_or(0);
    let infos = counts.get("info").copied().unwrap_or(0);

    // Calculate confidence score (0-100)
    let blocking_weight = (blocking * 15) as i32;
    let warning_weight = (warnings * 5) as i32;
    let info_weight = infos as i32;
    let deduction = blocking_weight + warning_weight + info_weight;
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

    let mut body = String::new();
    let _ = writeln!(body, "## 🦕 Codasaurus Review");
    let _ = writeln!(body);
    let _ = writeln!(
        body,
        "**{}** | 🔴 **{} blocking** | 🟡 **{} warnings** | 🔵 **{} info**",
        verdict, blocking, warnings, infos
    );
    let _ = writeln!(body, "**Review confidence:** {} {}%", score_emoji, confidence);
    let _ = writeln!(body);

    // Group findings by file, then by severity
    use std::collections::BTreeMap;
    let mut by_file: BTreeMap<String, Vec<&Finding>> = BTreeMap::new();
    for f in &findings.findings {
        by_file.entry(f.file.clone()).or_default().push(f);
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

    // Collapsible detailed breakdown by severity
    let _ = writeln!(body, "---");
    let _ = writeln!(body);

    if blocking > 0 {
        let _ = writeln!(body, "<details><summary>🔴 **{} Blocking**</summary>", blocking);
        let _ = writeln!(body);
        for f in &findings.findings {
            if f.severity == "blocking" {
                let _ = writeln!(body, "- **`{}:{}`** — {}", f.file, f.line, f.message);
                if let Some(ref s) = f.suggestion {
                    let _ = writeln!(body, "  - > 💡 {}", s);
                }
            }
        }
        let _ = writeln!(body, "</details>");
        let _ = writeln!(body);
    }

    if warnings > 0 {
        let _ = writeln!(body, "<details><summary>🟡 **{} Warnings**</summary>", warnings);
        let _ = writeln!(body);
        for f in &findings.findings {
            if f.severity == "warning" {
                let _ = writeln!(body, "- **`{}:{}`** — {}", f.file, f.line, f.message);
                if let Some(ref s) = f.suggestion {
                    let _ = writeln!(body, "  - > 💡 {}", s);
                }
            }
        }
        let _ = writeln!(body, "</details>");
        let _ = writeln!(body);
    }

    if infos > 0 {
        let _ = writeln!(body, "<details><summary>🔵 **{} Info**</summary>", infos);
        let _ = writeln!(body);
        for f in &findings.findings {
            if f.severity != "blocking" && f.severity != "warning" {
                let _ = writeln!(body, "- **`{}:{}`** — {}", f.file, f.line, f.message);
            }
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
            let _ = writeln!(body, "| {} | {} | {} |", check.name, check.status, check.details);
        }
        let _ = writeln!(body);
    }

    // Footer
    let _ = writeln!(body, "---");
    let _ = writeln!(
        body,
        "<sub>🦕 Reviewed by [Codasaurus](https://github.com/lohitkolluri/codasaurus) — `{}`</sub>",
        repo_name
    );
    let _ = writeln!(body);

    body
}

/// Generate and post an LLM-powered PR summary as a comment
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
) -> Result<()> {
    // Build a summary from the findings
    let mut findings_text = String::new();
    for f in &findings.findings {
        use std::fmt::Write;
        let _ = writeln!(findings_text, "- {}: {} (line {})", f.severity, f.message, f.line);
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

    let comment = serde_json::json!({"body": summary_body});
    let _: serde_json::Value = client
        .post(format!(
            "https://api.github.com/repos/{}/issues/{}/comments",
            repo_name, pr_number
        ))
        .header("Authorization", auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .json(&comment)
        .send()
        .await?
        .json()
        .await?;

    Ok(())
}
