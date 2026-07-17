use crate::bot::WebhookPayload;
use crate::detectors::{self, Finding, Findings};
use anyhow::Result;

pub async fn review_pr(token: &str, payload: &WebhookPayload) -> Result<()> {
    let pr = match &payload.pull_request {
        Some(p) => p,
        None => return Ok(()),
    };

    let repo_name = pr["head"]["repo"]["full_name"]
        .as_str()
        .unwrap_or("unknown");
    let pr_number = pr["number"].as_i64().unwrap_or(0);
    let pr_title = pr["title"].as_str().unwrap_or("").to_string();
    let pr_body = pr["body"].as_str().unwrap_or("").to_string();
    let _head_sha = pr["head"]["sha"].as_str().unwrap_or("");

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
                    .extend(detectors::run_all(&[p], &crate::config::Config::default()).findings);
            }
        }
    }

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

    // If no findings, post a positive comment
    if review_comments.is_empty() {
        let body = "## 🦕 Codasaurus Review\n\n✅ No issues found. Looks good!";
        let comment = serde_json::json!({"body": body});
        let _: serde_json::Value = client
            .post(format!(
                "https://api.github.com/repos/{}/issues/{}/comments",
                repo_name, pr_number
            ))
            .header("Authorization", &auth_header)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus/0.1.0")
            .json(&comment)
            .send()
            .await?
            .json()
            .await?;
        return Ok(());
    }

    let body = build_review_body(
        &findings,
        total_findings,
        has_blocking,
        repo_name,
    );

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

    // If inline review failed (e.g. line numbers don't match), fall back to a single issue comment
    if !resp.status().is_success() {
        let fallback_body = serde_json::json!({"body": body});
        let _: serde_json::Value = client
            .post(format!(
                "https://api.github.com/repos/{}/issues/{}/comments",
                repo_name, pr_number
            ))
            .header("Authorization", &auth_header)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus/0.1.0")
            .json(&fallback_body)
            .send()
            .await?
            .json()
            .await?;
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
    let mut body = format!(
        "{} **{}** `{}` — {}",
        icon, finding.detector, finding.severity, finding.message
    );
    if let Some(s) = &finding.suggestion {
        use std::fmt::Write;
        let _ = write!(body, "\n\n> 💡 {}", s);
    }
    if let Some(c) = &finding.codemod {
        use std::fmt::Write;
        let _ = write!(body, "\n\n```\n{}\n```", c);
    }
    body
}

fn build_review_body(
    findings: &Findings,
    total: usize,
    has_blocking: bool,
    repo_name: &str,
) -> String {
    use std::fmt::Write;

    let counts = findings.count_by_severity();
    let blocking = counts.get("blocking").copied().unwrap_or(0);
    let warnings = counts.get("warning").copied().unwrap_or(0);
    let infos = counts.get("info").copied().unwrap_or(0);

    let verdict = if has_blocking {
        "⛔ Changes requested"
    } else if warnings > 0 {
        "⚠️ Review with suggestions"
    } else {
        "ℹ️ Info only"
    };

    let mut body = format!(
        "## 🦕 Codasaurus Review\n\n**{}** — {} issue(s): {} blocking, {} warnings, {} info\n\n---\n",
        verdict, total, blocking, warnings, infos
    );

    // Group findings by severity in a single pass
    let mut blocking_items = String::new();
    let mut warning_items = String::new();
    let mut info_items = String::new();
    for f in &findings.findings {
        let line = match f.severity {
            "blocking" => &mut blocking_items,
            "warning" => &mut warning_items,
            _ => &mut info_items,
        };
        let _ = writeln!(line, "- `{}:{}` — {}", f.file, f.line, f.message);
    }

    if blocking > 0 {
        let _ = write!(body, "\n### 🔴 Blocking\n{}", blocking_items);
    }
    if warnings > 0 {
        let _ = write!(body, "\n### 🟡 Warnings\n{}", warning_items);
    }
    if infos > 0 {
        let _ = write!(body, "\n### 🔵 Info\n{}", info_items);
    }

    let _ = write!(
        body,
        "\n---\n_Powered by [Codasaurus](https://github.com/lohitkolluri/codasaurus) — reviewing `{}`_\n",
        repo_name
    );

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
