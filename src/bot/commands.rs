//! Slash commands: parse `@codasaurus …` and spawn handlers.

use std::time::Duration;
use tokio::time::timeout;

use super::auth::get_installation_token;
use super::github_extra;
use super::markdown;
use super::review::{fetch_pull_request, review_pr};
use super::worker::release_claim_best_effort;
use super::{
    bot_db_pool, pr_lock, prune_pr_lock, WebhookContext, WebhookPayload, REVIEW_PERMITS, USER_AGENT,
};
use crate::learning::store::LearningStore;

#[derive(Debug, Clone)]
pub(crate) enum BotCommand {
    Review,
    Describe,
    Improve,
    Summarize,
    Labels,
    Changelog,
    Security,
    AddDocs,
    Ask(String),
    Ignore(Option<String>),
    Help,
}

pub(crate) fn parse_bot_command(body: &str) -> Option<BotCommand> {
    let lower = body.to_ascii_lowercase();
    let mentions = ["@codasaurus", "@codasaurus-bot"];
    if !mentions.iter().any(|m| lower.contains(m)) {
        return None;
    }
    if lower.contains("@codasaurus help") || lower.contains("@codasaurus-bot help") {
        return Some(BotCommand::Help);
    }
    if lower.contains("add_docs") || lower.contains("add-docs") || lower.contains("add docs") {
        return Some(BotCommand::AddDocs);
    }
    if lower.contains("changelog") {
        return Some(BotCommand::Changelog);
    }
    if lower.contains("security") {
        return Some(BotCommand::Security);
    }
    if lower.contains("labels") || lower.contains("label") {
        return Some(BotCommand::Labels);
    }
    if lower.contains("summarize") || lower.contains("summary") {
        return Some(BotCommand::Summarize);
    }
    if lower.contains("describe") {
        return Some(BotCommand::Describe);
    }
    if lower.contains("improve") {
        return Some(BotCommand::Improve);
    }
    if lower.contains(" ignore") || lower.contains(" dismiss") {
        return Some(BotCommand::Ignore(extract_ignore_fingerprint(body)));
    }
    if let Some(q) = extract_ask_question(body) {
        return Some(BotCommand::Ask(q));
    }
    if lower.contains("review") {
        return Some(BotCommand::Review);
    }
    None
}

fn extract_ask_question(body: &str) -> Option<String> {
    for prefix in ["@codasaurus ask ", "@codasaurus-bot ask "] {
        if let Some(rest) = body.split(prefix).nth(1) {
            let q = rest.trim();
            if !q.is_empty() {
                return Some(q.to_string());
            }
        }
        // case-insensitive fallback
        let lower = body.to_ascii_lowercase();
        let p = prefix.to_ascii_lowercase();
        if let Some(idx) = lower.find(&p) {
            let q = body[idx + prefix.len()..].trim();
            if !q.is_empty() {
                return Some(q.to_string());
            }
        }
    }
    None
}

fn extract_ignore_fingerprint(body: &str) -> Option<String> {
    for prefix in [
        "@codasaurus ignore ",
        "@codasaurus-bot ignore ",
        "@codasaurus dismiss ",
        "@codasaurus-bot dismiss ",
    ] {
        if let Some(rest) = body.split(prefix).nth(1) {
            let fp = rest.split_whitespace().next().unwrap_or("").trim();
            if !fp.is_empty() && fp.len() >= 8 {
                return Some(fp.to_string());
            }
        }
        let lower = body.to_ascii_lowercase();
        let p = prefix.to_ascii_lowercase();
        if let Some(idx) = lower.find(&p) {
            let rest = &body[idx + prefix.len()..];
            let fp = rest.split_whitespace().next().unwrap_or("").trim();
            if !fp.is_empty() && fp.len() >= 8 {
                return Some(fp.to_string());
            }
        }
    }
    None
}

pub(crate) async fn handle_bot_command(
    ctx: WebhookContext,
    pr_number: i64,
    cmd: BotCommand,
    timeout_secs: u64,
) {
    match cmd {
        BotCommand::Review => spawn_review(ctx, pr_number, timeout_secs).await,
        BotCommand::Ignore(fp) => spawn_ignore_comment(ctx, pr_number, fp).await,
        BotCommand::Help => spawn_simple_comment(ctx, pr_number, markdown::help_body()).await,
        BotCommand::Describe => spawn_describe(ctx, pr_number, timeout_secs).await,
        BotCommand::Improve => spawn_improve(ctx, pr_number, timeout_secs).await,
        BotCommand::Ask(q) => spawn_ask(ctx, pr_number, q, timeout_secs).await,
        BotCommand::Summarize => spawn_summarize(ctx, pr_number, timeout_secs).await,
        BotCommand::Labels => spawn_labels(ctx, pr_number, timeout_secs).await,
        BotCommand::Changelog => spawn_changelog(ctx, pr_number, timeout_secs).await,
        BotCommand::Security => spawn_security(ctx, pr_number, timeout_secs).await,
        BotCommand::AddDocs => spawn_add_docs(ctx, pr_number, timeout_secs).await,
    }
}

async fn post_issue_comment(token: &str, repo: &str, pr: i64, body: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("https://api.github.com/repos/{repo}/issues/{pr}/comments");
    crate::retry::github_request(&crate::retry::RetryConfig::api_default(), "post_comment", || {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .json(&serde_json::json!({"body": body}))
    })
    .await?;
    Ok(())
}

async fn spawn_simple_comment(ctx: WebhookContext, pr_number: i64, body: String) {
    match get_installation_token(&ctx.cfg, ctx.inst_id).await {
        Ok(token) => {
            if let Err(e) = post_issue_comment(&token, &ctx.repo_full_name, pr_number, &body).await
            {
                tracing::error!(error = %e, "failed to post comment");
            }
        }
        Err(e) => tracing::error!(error = %e, "auth error"),
    }
}

async fn spawn_describe(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("");
        let body = pr["body"].as_str().unwrap_or("");
        let pool = bot_db_pool();
        let files_hint = fetch_changed_paths_hint(&token, &ctx.repo_full_name, pr_number)
            .await
            .unwrap_or_default();
        let text = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            match crate::llm::describe_pr(title, body, &files_hint, &llm).await {
                Ok(s) => format!("### Codasaurus describe\n\n{s}"),
                Err(e) => format!(
                    "### Codasaurus describe\n\n**{title}**\n\n{}\n\n_LLM unavailable: {e}_",
                    body.chars().take(500).collect::<String>()
                ),
            }
        } else {
            format!(
                "### Codasaurus describe\n\n**Title:** {title}\n\n{}\n\n_Configure an LLM key for richer summaries._",
                body.chars().take(800).collect::<String>()
            )
        };
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

async fn spawn_improve(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let pool = bot_db_pool();
    let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await else {
        // No LLM — fall back to full static review (still surfaces codemods).
        spawn_review(ctx, pr_number, timeout_secs).await;
        return;
    };

    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };

    let ctx_fallback = ctx.clone();
    let result = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("").to_string();
        let body = pr["body"].as_str().unwrap_or("").to_string();
        let head_ref = pr["head"]["ref"].as_str().unwrap_or("").to_string();

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let auth = format!("Bearer {token}");
        let files_url = format!(
            "https://api.github.com/repos/{}/pulls/{}/files?per_page=100",
            ctx.repo_full_name, pr_number
        );
        let files: Vec<serde_json::Value> = client
            .get(&files_url)
            .header("Authorization", &auth)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut diff = String::new();
        for f in files.iter().take(40) {
            let name = f["filename"].as_str().unwrap_or("?");
            let patch = f["patch"].as_str().unwrap_or("");
            if patch.is_empty() {
                continue;
            }
            let _ = std::fmt::Write::write_fmt(
                &mut diff,
                format_args!("--- a/{name}\n+++ b/{name}\n{patch}\n"),
            );
            if diff.len() > 24_000 {
                break;
            }
        }
        if diff.is_empty() {
            post_issue_comment(
                &token,
                &ctx.repo_full_name,
                pr_number,
                "### Codasaurus improve\n\nNo textual diffs available to improve.",
            )
            .await?;
            return Ok::<_, anyhow::Error>(());
        }

        let review_ctx = crate::llm::ReviewContext {
            repo: Some(ctx.repo_full_name.clone()),
            branch: Some(head_ref),
            pr_title: Some(title.clone()),
            pr_description: Some(body.chars().take(2_000).collect()),
            linked_issues: Vec::new(),
            related_prs: Vec::new(),
            repo_context: Some(format!(
                "Improve mode: suggest concrete code fixes for PR `{title}`."
            )),
        };

        let output = crate::llm::review_diff(&diff, &llm, Some(&review_ctx)).await?;
        let mut text = String::from("### Codasaurus improve\n\n");
        if let Some(summary) = output.summary.as_deref().filter(|s| !s.is_empty()) {
            let _ = std::fmt::Write::write_fmt(&mut text, format_args!("{summary}\n\n"));
        } else if !output.verdict.is_empty() {
            let _ = std::fmt::Write::write_fmt(
                &mut text,
                format_args!("**Verdict:** {}\n\n", output.verdict),
            );
        }
        if output.issues.is_empty() {
            text.push_str("_No improvement suggestions from the model._\n");
        } else {
            text.push_str("| File | Line | Severity | Suggestion |\n| --- | ---: | --- | --- |\n");
            for issue in output.issues.iter().take(20) {
                let sev = &issue.severity;
                let sug = issue
                    .suggestion
                    .as_deref()
                    .unwrap_or(&issue.description)
                    .replace('|', "\\|")
                    .chars()
                    .take(160)
                    .collect::<String>();
                let _ = std::fmt::Write::write_fmt(
                    &mut text,
                    format_args!("| `{}` | {} | `{sev}` | {sug} |\n", issue.file, issue.line),
                );
            }
        }
        text.push_str(
            "\n<details><summary>Commands</summary>\n\n`@codasaurus review` · `@codasaurus ask …`\n\n</details>",
        );
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await?;
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => tracing::info!(pr = pr_number, "improve completed"),
        Ok(Err(e)) => {
            tracing::warn!(pr = pr_number, error = %e, "improve failed; falling back to review");
            drop(_permit);
            spawn_review(ctx_fallback, pr_number, timeout_secs).await;
        }
        Err(_) => tracing::error!(pr = pr_number, "improve timed out"),
    }
}

async fn spawn_ask(ctx: WebhookContext, pr_number: i64, question: String, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("");
        let body = pr["body"].as_str().unwrap_or("");
        let pool = bot_db_pool();
        let files_hint = fetch_changed_paths_hint(&token, &ctx.repo_full_name, pr_number)
            .await
            .unwrap_or_default();
        let answer = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            crate::llm::ask_about_pr(title, body, &question, &files_hint, &llm)
                .await
                .unwrap_or_else(|e| format!("Could not answer: {e}"))
        } else {
            "Configure an LLM API key to use `@codasaurus ask`.".into()
        };
        let text = format!("### Codasaurus ask\n\n> {question}\n\n{answer}");
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

/// Lightweight changed-file list for ask/describe prompts (no full patches).
async fn fetch_changed_paths_hint(
    token: &str,
    repo: &str,
    pr_number: i64,
) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let auth = format!("Bearer {token}");
    let files_url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100");
    let files: Vec<serde_json::Value> = client
        .get(&files_url)
        .header("Authorization", &auth)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut out = String::new();
    for f in files.iter().take(60) {
        let name = f["filename"].as_str().unwrap_or("?");
        let status = f["status"].as_str().unwrap_or("modified");
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{status}\t{name}\n"));
    }
    Ok(out)
}

async fn spawn_summarize(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("");
        let body = pr["body"].as_str().unwrap_or("");
        let pool = bot_db_pool();
        let text = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            match crate::llm::summarize_pr(
                title,
                body,
                "Write a 3-5 sentence executive summary of this PR. No markdown tables.",
                &llm,
            )
            .await
            {
                Ok(s) => format!("### Codasaurus summarize\n\n{s}"),
                Err(e) => format!(
                    "### Codasaurus summarize\n\n**{title}**\n\n{}\n\n_LLM unavailable: {e}_",
                    body.chars().take(400).collect::<String>()
                ),
            }
        } else {
            format!(
                "### Codasaurus summarize\n\n**{title}**\n\n{}",
                body.chars().take(600).collect::<String>()
            )
        };
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

async fn spawn_labels(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let auth = format!("Bearer {token}");
        let files_url = format!(
            "https://api.github.com/repos/{}/pulls/{}/files?per_page=100",
            ctx.repo_full_name, pr_number
        );
        let files: Vec<serde_json::Value> = client
            .get(&files_url)
            .header("Authorization", &auth)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let paths: Vec<String> = files
            .iter()
            .filter_map(|f| f["filename"].as_str().map(str::to_string))
            .collect();
        let labels = github_extra::suggest_labels(&paths, &[]);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            auth.parse().map_err(|e| anyhow::anyhow!("{e}"))?,
        );
        headers.insert(
            reqwest::header::ACCEPT,
            "application/vnd.github+json".parse().unwrap(),
        );
        github_extra::apply_labels(&client, &headers, &ctx.repo_full_name, pr_number, &labels)
            .await?;
        let text = format!(
            "### Codasaurus labels\n\nApplied: {}\n\n_Based on changed paths._",
            labels
                .iter()
                .map(|l| format!("`{l}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

async fn spawn_changelog(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("Update").to_string();
        let body = pr["body"].as_str().unwrap_or("").to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let auth = format!("Bearer {token}");
        let files_url = format!(
            "https://api.github.com/repos/{}/pulls/{}/files?per_page=100",
            ctx.repo_full_name, pr_number
        );
        let files: Vec<serde_json::Value> = client
            .get(&files_url)
            .header("Authorization", &auth)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .unwrap_or_default();
        let file_list: String = files
            .iter()
            .filter_map(|f| f["filename"].as_str())
            .take(15)
            .map(|p| format!("- `{p}`"))
            .collect::<Vec<_>>()
            .join("\n");

        let pool = bot_db_pool();
        let sections = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            let ctx_text = format!(
                "Changed files:\n{file_list}\n\nWrite a Keep a Changelog style draft with sections \
                 ### Added, ### Changed, ### Fixed, ### Security (omit empty). Short bullets only."
            );
            crate::llm::summarize_pr(&title, &body, &ctx_text, &llm)
                .await
                .unwrap_or_else(|_| heuristic_changelog(&title, &file_list))
        } else {
            heuristic_changelog(&title, &file_list)
        };

        let text = format!(
            "### Codasaurus changelog\n\n{sections}\n\n<details><summary>Files</summary>\n\n{file_list}\n\n</details>"
        );
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

fn heuristic_changelog(title: &str, file_list: &str) -> String {
    let lower = title.to_ascii_lowercase();
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut fixed = Vec::new();
    let mut security = Vec::new();
    if lower.contains("fix") || lower.contains("bug") {
        fixed.push(format!("- {title}"));
    } else if lower.contains("feat") || lower.contains("add") {
        added.push(format!("- {title}"));
    } else {
        changed.push(format!("- {title}"));
    }
    if file_list.contains("security") || file_list.contains(".tf") {
        security.push("- Review IaC / security-related path changes".to_string());
    }
    let mut out = String::new();
    if !added.is_empty() {
        out.push_str("### Added\n");
        out.push_str(&added.join("\n"));
        out.push_str("\n\n");
    }
    if !changed.is_empty() {
        out.push_str("### Changed\n");
        out.push_str(&changed.join("\n"));
        out.push_str("\n\n");
    }
    if !fixed.is_empty() {
        out.push_str("### Fixed\n");
        out.push_str(&fixed.join("\n"));
        out.push_str("\n\n");
    }
    if !security.is_empty() {
        out.push_str("### Security\n");
        out.push_str(&security.join("\n"));
        out.push('\n');
    }
    if out.is_empty() {
        format!("### Changed\n- {title}\n")
    } else {
        out
    }
}

async fn spawn_add_docs(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("");
        let body = pr["body"].as_str().unwrap_or("");
        let files_hint = fetch_changed_paths_hint(&token, &ctx.repo_full_name, pr_number)
            .await
            .unwrap_or_default();
        let pool = bot_db_pool();
        let text = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            let ctx_text = format!(
                "Changed files:\n{files_hint}\n\nSuggest documentation updates (README / docs/) \
                 for public APIs or behavior changes. Output markdown stubs only — do not claim files were committed."
            );
            match crate::llm::describe_pr(title, body, &ctx_text, &llm).await {
                Ok(s) => format!("### Codasaurus add_docs\n\n{s}\n\n_Suggestions only — not auto-committed._"),
                Err(e) => format!("### Codasaurus add_docs\n\nCould not generate docs suggestions: {e}"),
            }
        } else {
            format!(
                "### Codasaurus add_docs\n\n**{title}**\n\nChanged paths:\n```\n{files_hint}\n```\n\n\
                 Configure an LLM key for drafted README/docs stubs."
            )
        };
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

async fn spawn_security(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let auth = format!("Bearer {token}");
        let files_url = format!(
            "https://api.github.com/repos/{}/pulls/{}/files?per_page=100",
            ctx.repo_full_name, pr_number
        );
        let files: Vec<serde_json::Value> = client
            .get(&files_url)
            .header("Authorization", &auth)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut parsed = Vec::new();
        for f in files.iter().take(80) {
            let name = f["filename"].as_str().unwrap_or("");
            let patch = f["patch"].as_str().unwrap_or("");
            if name.is_empty() || patch.is_empty() {
                continue;
            }
            // Reconstruct rough file content from patch added lines for secret scan.
            let mut content = String::new();
            for line in patch.lines() {
                if let Some(rest) = line.strip_prefix('+') {
                    if !rest.starts_with('+') {
                        content.push_str(rest);
                        content.push('\n');
                    }
                }
            }
            if content.is_empty() {
                continue;
            }
            if let Ok(pf) = crate::parser::parse_file(name, &content) {
                parsed.push(pf);
            }
        }

        let mut findings = crate::detectors::security::detect_secrets(&parsed);
        findings.extend(crate::detectors::vulnerabilities::detect(&parsed));
        findings.retain(|f| {
            matches!(
                f.detector.as_str(),
                "secrets" | "vulnerabilities" | "todo-leaks"
            ) || f.severity == "blocking"
        });

        let mut text = String::from("### Codasaurus security\n\n");
        if findings.is_empty() {
            text.push_str("No secrets or known vulnerability signals in the scanned diff.\n");
        } else {
            text.push_str("| Severity | Detector | File | Message |\n| --- | --- | --- | --- |\n");
            for f in findings.iter().take(25) {
                let msg = f.message.replace('|', "\\|").chars().take(120).collect::<String>();
                let _ = std::fmt::Write::write_fmt(
                    &mut text,
                    format_args!(
                        "| `{}` | `{}` | `{}:{}` | {msg} |\n",
                        f.severity, f.detector, f.file, f.line
                    ),
                );
            }
        }
        text.push_str("\n_Run `@codasaurus review` for the full detector suite._");
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

async fn spawn_review(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let repo_name = ctx.repo_full_name.clone();
    let lock = pr_lock(&repo_name, pr_number).await;
    let _guard = lock.lock().await;

    let head_sha_holder = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let head_sha_slot = head_sha_holder.clone();
    match timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        if pr_number <= 0 || ctx.repo_full_name == "unknown" {
            anyhow::bail!("invalid repository or PR number");
        }
        let pr_data = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        if let Some(sha) = pr_data["head"]["sha"].as_str() {
            *head_sha_slot.lock().await = sha.to_string();
        }
        // Force review even if draft when manually requested
        let mut pr_data = pr_data;
        if let Some(obj) = pr_data.as_object_mut() {
            obj.insert("draft".into(), serde_json::json!(false));
        }
        let wrapped = WebhookPayload {
            action: String::new(),
            pull_request: Some(pr_data),
            repo: None,
            installation: None,
            comment: None,
            issue: None,
            repositories: None,
            repositories_added: None,
        };
        review_pr(&token, &ctx.repo_full_name, &wrapped).await
    })
    .await
    {
        Ok(Ok(())) => tracing::info!(pr = pr_number, "comment-triggered review completed"),
        Ok(Err(e)) => {
            tracing::error!(pr = pr_number, error = %e, "comment-triggered review failed");
            let sha = head_sha_holder.lock().await.clone();
            release_claim_best_effort(&repo_name, pr_number, &sha).await;
        }
        Err(_) => {
            tracing::error!(pr = pr_number, "comment-triggered review timed out");
            let sha = head_sha_holder.lock().await.clone();
            release_claim_best_effort(&repo_name, pr_number, &sha).await;
        }
    }

    drop(_guard);
    prune_pr_lock(&repo_name, pr_number).await;
}

async fn spawn_ignore_comment(ctx: WebhookContext, pr_number: i64, fingerprint: Option<String>) {
    match timeout(Duration::from_secs(120), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;

        let body = if let Some(ref fp) = fingerprint {
            if let Some(pool) = bot_db_pool() {
                let store = LearningStore::from_pool(pool);
                store
                    .dismiss_fingerprint(fp, "manual", &ctx.repo_full_name, "dismissed via comment")
                    .await?;
                format!(
                    "### Dismissed\n\nFinding `{fp}` will be filtered on future reviews."
                )
            } else {
                format!(
                    "### Could not dismiss\n\nDatabase unavailable — could not persist `{fp}`."
                )
            }
        } else {
            "### Ignore\n\nReply with `@codasaurus ignore <fingerprint>` (see the fingerprint on each finding comment)."
                .to_string()
        };

        let url = format!(
            "https://api.github.com/repos/{}/issues/{}/comments",
            ctx.repo_full_name, pr_number
        );
        crate::retry::github_request(&crate::retry::RetryConfig::api_default(), "post_ignore_comment", || {
            client
                .post(&url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", USER_AGENT)
                .json(&serde_json::json!({"body": body}))
        })
        .await?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    {
        Ok(Ok(())) => tracing::info!(pr = pr_number, "ignore command handled"),
        Ok(Err(e)) => tracing::error!(pr = pr_number, error = %e, "ignore command failed"),
        Err(_) => tracing::error!(pr = pr_number, "ignore command timed out"),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_review_and_add_docs() {
        assert!(matches!(
            parse_bot_command("@codasaurus review"),
            Some(BotCommand::Review)
        ));
        assert!(matches!(
            parse_bot_command("@codasaurus add_docs please"),
            Some(BotCommand::AddDocs)
        ));
        assert!(matches!(
            parse_bot_command("@codasaurus changelog"),
            Some(BotCommand::Changelog)
        ));
    }

    #[test]
    fn parses_ask_and_ignore() {
        let cmd = parse_bot_command("@codasaurus ask why is this flaky?");
        match cmd {
            Some(BotCommand::Ask(q)) => assert!(q.contains("flaky")),
            other => panic!("expected Ask, got {other:?}"),
        }
        let cmd = parse_bot_command("@codasaurus ignore abcdef012345");
        match cmd {
            Some(BotCommand::Ignore(Some(fp))) => assert!(fp.starts_with("abcdef")),
            other => panic!("expected Ignore, got {other:?}"),
        }
    }

    #[test]
    fn ignores_unrelated_comments() {
        assert!(parse_bot_command("lgtm thanks").is_none());
    }
}
