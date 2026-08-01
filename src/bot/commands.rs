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
    Similar,
    Fix(Option<String>),
    Impact,
    Digest,
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
    if lower.contains("changelog") || lower.contains("update_changelog") {
        return Some(BotCommand::Changelog);
    }
    if lower.contains("similar") || lower.contains("related_prs") || lower.contains("related-prs") {
        return Some(BotCommand::Similar);
    }
    if lower.contains("impact") || lower.contains("blast-radius") || lower.contains("blast_radius")
    {
        return Some(BotCommand::Impact);
    }
    if lower.contains("digest") || lower.contains("weekly") {
        return Some(BotCommand::Digest);
    }
    if lower.contains("@codasaurus fix")
        || lower.contains("@codasaurus-bot fix")
        || lower.contains(" autofix")
        || lower.contains(" apply_fix")
    {
        return Some(BotCommand::Fix(extract_fix_fingerprint(body)));
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

fn extract_fix_fingerprint(body: &str) -> Option<String> {
    for prefix in [
        "@codasaurus fix ",
        "@codasaurus-bot fix ",
        "@codasaurus autofix ",
        "@codasaurus apply_fix ",
    ] {
        if let Some(rest) = body.split(prefix).nth(1) {
            let fp = rest.split_whitespace().next().unwrap_or("").trim();
            if !fp.is_empty() && fp.len() >= 8 && !fp.starts_with('<') {
                return Some(fp.to_string());
            }
        }
        let lower = body.to_ascii_lowercase();
        let p = prefix.to_ascii_lowercase();
        if let Some(idx) = lower.find(&p) {
            let rest = &body[idx + prefix.len()..];
            let fp = rest.split_whitespace().next().unwrap_or("").trim();
            if !fp.is_empty() && fp.len() >= 8 && !fp.starts_with('<') {
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
        BotCommand::Similar => spawn_similar(ctx, pr_number, timeout_secs).await,
        BotCommand::Fix(fp) => spawn_fix(ctx, pr_number, fp, timeout_secs).await,
        BotCommand::Impact => spawn_impact(ctx, pr_number, timeout_secs).await,
        BotCommand::Digest => spawn_digest(ctx, pr_number).await,
    }
}

async fn post_issue_comment(token: &str, repo: &str, pr: i64, body: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("https://api.github.com/repos/{repo}/issues/{pr}/comments");
    crate::retry::github_request(
        &crate::retry::RetryConfig::api_default(),
        "post_comment",
        || {
            client
                .post(&url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", USER_AGENT)
                .json(&serde_json::json!({"body": body}))
        },
    )
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
                Ok(s) => format!(
                    "### Codasaurus describe\n\n{s}\n\n---\n{}",
                    crate::bot::markdown::commands_details()
                ),
                Err(e) => format!(
                    "### Codasaurus describe\n\n**{title}**\n\n{}\n\n> LLM unavailable: `{e}`\n\n---\n{}",
                    body.chars().take(500).collect::<String>(),
                    crate::bot::markdown::commands_details()
                ),
            }
        } else {
            format!(
                "### Codasaurus describe\n\n**{title}**\n\n{}\n\n> Configure an LLM key for richer summaries.\n\n---\n{}",
                body.chars().take(800).collect::<String>(),
                crate::bot::markdown::commands_details()
            )
        };
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await?;

        let mut update_body = false;
        if let Some(pool) = pool {
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "update_pr_description").await {
                update_body = matches!(
                    v.to_ascii_lowercase().as_str(),
                    "true" | "1" | "yes" | "on"
                );
            }
            if let Ok(Some(repo)) =
                crate::db::repos::get_repo_by_full_name(pool, &ctx.repo_full_name).await
            {
                if let Some(cfg) = repo.config_json.as_deref() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(cfg) {
                        if let Some(b) = val.get("update_pr_description").and_then(|v| v.as_bool())
                        {
                            update_body = b;
                        }
                    }
                }
            }
        }
        if update_body {
            let plain = text
                .strip_prefix("### Codasaurus describe\n\n")
                .unwrap_or(&text);
            let _ = patch_pr_body(&token, &ctx.repo_full_name, pr_number, plain).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await;
}

async fn patch_pr_body(token: &str, repo: &str, pr_number: i64, body: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}");
    let resp = client
        .patch(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", USER_AGENT)
        .json(&serde_json::json!({ "body": body }))
        .send()
        .await?;
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "update_pr_description failed");
    }
    Ok(())
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
        let known_paths: Vec<String> = files
            .iter()
            .filter_map(|f| f["filename"].as_str().map(str::to_string))
            .collect();
        let file_contents: Vec<(String, String)> = files
            .iter()
            .filter_map(|f| {
                let name = f["filename"].as_str()?.to_string();
                let patch = f["patch"].as_str().unwrap_or("").to_string();
                if patch.is_empty() {
                    None
                } else {
                    Some((name, patch))
                }
            })
            .collect();
        let issues = crate::bot::provenance::reverify_llm_issues(
            &output.issues,
            &known_paths,
            &file_contents,
        );
        let mut text = String::from("### Codasaurus improve\n\n");
        if let Some(summary) = output.summary.as_deref().filter(|s| !s.is_empty()) {
            let _ = std::fmt::Write::write_fmt(&mut text, format_args!("{summary}\n\n"));
        } else if !output.verdict.is_empty() {
            let _ = std::fmt::Write::write_fmt(
                &mut text,
                format_args!("**Verdict:** {}\n\n", output.verdict),
            );
        }
        if issues.is_empty() {
            text.push_str("> No improvement suggestions after path re-verify.\n");
        } else {
            text.push_str(
                "| File | Line | Severity | Suggestion | Source |\n| --- | ---: | --- | --- | --- |\n",
            );
            for issue in issues.iter().take(20) {
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
                    format_args!(
                        "| `{}` | {} | `{sev}` | {sug} | `llm` |\n",
                        issue.file, issue.line
                    ),
                );
            }
        }
        text.push('\n');
        text.push_str(&crate::bot::markdown::commands_details());
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
        let text = format!(
            "### Codasaurus ask\n\n**Question**\n\n> {question}\n\n**Answer**\n\n{answer}\n\n---\n{}",
            crate::bot::markdown::commands_details()
        );
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
    let files_url =
        format!("https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100");
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
                Ok(s) => format!(
                    "### Codasaurus summarize\n\n{s}\n\n---\n{}",
                    crate::bot::markdown::commands_details()
                ),
                Err(e) => format!(
                    "### Codasaurus summarize\n\n**{title}**\n\n{}\n\n> LLM unavailable: `{e}`",
                    body.chars().take(400).collect::<String>()
                ),
            }
        } else {
            format!(
                "### Codasaurus summarize\n\n**{title}**\n\n{}\n\n> Configure an LLM key for richer summaries.",
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
            "### Codasaurus labels\n\n**Applied**\n\n{}\n\n<sub>Suggested from changed paths.</sub>",
            if labels.is_empty() {
                "_No labels suggested._".into()
            } else {
                labels
                    .iter()
                    .map(|l| format!("- `{l}`"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
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
        let existing = {
            let mut headers = reqwest::header::HeaderMap::new();
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&auth) {
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
            );
            headers.insert(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static(USER_AGENT),
            );
            let head = pr["head"]["sha"].as_str().unwrap_or("HEAD");
            match crate::bot::github_files::fetch_first_existing(
                &client,
                &headers,
                &ctx.repo_full_name,
                &["CHANGELOG.md", "CHANGELOG", "docs/CHANGELOG.md"],
                head,
            )
            .await
            {
                Ok(Some((_path, content))) => content.chars().take(2000).collect::<String>(),
                _ => String::new(),
            }
        };
        let sections = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            crate::llm::changelog_pr(&title, &body, &file_list, &existing, &llm)
                .await
                .unwrap_or_else(|_| heuristic_changelog(&title, &file_list))
        } else {
            heuristic_changelog(&title, &file_list)
        };

        let text = format!(
            "### Codasaurus changelog\n\n{sections}\n\n<details>\n<summary><strong>Files</strong></summary>\n\n{file_list}\n\n</details>\n\n<sub>Draft only — paste into CHANGELOG.md as needed.</sub>"
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
                Ok(s) => format!(
                    "### Codasaurus add_docs\n\n{s}\n\n> Suggestions only — not auto-committed.\n"
                ),
                Err(e) => format!(
                    "### Codasaurus add_docs\n\nCould not generate docs suggestions: `{e}`\n"
                ),
            }
        } else {
            format!(
                "### Codasaurus add_docs\n\n**{title}**\n\nChanged paths:\n```\n{files_hint}\n```\n\n\
                 > Configure an LLM key for drafted README/docs stubs.\n"
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
        // Honor offline / air-gap before OSV / registry egress.
        let offline = if let Some(pool) = crate::bot::CONFIG_POOL.get() {
            let db_off = crate::db::config::get_config(pool, "offline_mode")
                .await
                .ok()
                .flatten();
            let on = crate::bot::offline::offline_mode_from_env_and_db(db_off.as_deref());
            crate::registry::set_offline_mode(on);
            on
        } else {
            crate::bot::offline::offline_mode_from_env_and_db(None)
        };
        if !offline {
            findings.extend(crate::detectors::vulnerabilities::detect(&parsed));
        }
        findings.retain(|f| {
            matches!(
                f.detector.as_str(),
                "secrets" | "vulnerabilities" | "todo-leaks"
            ) || f.severity == "blocking"
        });

        let mut text = String::from("### Codasaurus security\n\n");
        if findings.is_empty() {
            text.push_str(
                "> **Clean** — no secrets or known vulnerability signals in the scanned diff.\n",
            );
        } else {
            text.push_str(
                "| Severity | Detector | Location | Message |\n| --- | --- | --- | --- |\n",
            );
            for f in findings.iter().take(25) {
                let msg = f
                    .message
                    .replace('|', "\\|")
                    .chars()
                    .take(120)
                    .collect::<String>();
                let _ = std::fmt::Write::write_fmt(
                    &mut text,
                    format_args!(
                        "| `{}` | `{}` | `{}:{}` | {msg} |\n",
                        f.severity, f.detector, f.file, f.line
                    ),
                );
            }
        }
        text.push_str("\n---\n");
        text.push_str(&crate::bot::markdown::commands_details());
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
            reaction: None,
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
                    .dismiss_fingerprint_for_repo(
                        fp,
                        "manual",
                        &ctx.repo_full_name,
                        "dismissed via comment",
                        Some(&ctx.repo_full_name),
                    )
                    .await?;
                format!(
                    "### Codasaurus dismiss\n\nFinding `{fp}` will be filtered on future reviews.\n\n<sub>`@codasaurus help`</sub>"
                )
            } else {
                format!(
                    "### Codasaurus dismiss\n\nDatabase unavailable — could not persist `{fp}`."
                )
            }
        } else {
            "### Codasaurus dismiss\n\nReply with `@codasaurus ignore <fingerprint>` (see each finding comment).\n"
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

async fn spawn_digest(ctx: WebhookContext, pr_number: i64) {
    match timeout(Duration::from_secs(60), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let body = if let Some(pool) = bot_db_pool() {
            let stats = crate::db::reviews::get_stats(pool)
                .await
                .unwrap_or_else(|_| serde_json::json!({}));
            let reviews = stats
                .get("reviews_last_7_days")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let dismissals = stats
                .get("dismissals_last_7_days")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let findings = stats
                .get("total_findings")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let pass = stats
                .get("pass_rate")
                .and_then(|v| v.as_f64())
                .map(|p| format!("{p:.0}%"))
                .unwrap_or_else(|| "-".into());
            format!(
                "### Codasaurus digest (7 days)\n\n\
                 | Metric | Value |\n\
                 | --- | --- |\n\
                 | Reviews | {reviews} |\n\
                 | Active findings (all time rows) | {findings} |\n\
                 | Dismissals | {dismissals} |\n\
                 | Pass rate (all time) | {pass} |\n\n\
                 Open the dashboard **Review analytics** panel for detector breakdowns.\n\n\
                 <sub>`@codasaurus help`</sub>"
            )
        } else {
            "### Codasaurus digest\n\n> Database unavailable.".into()
        };
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &body).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    {
        Ok(Ok(())) => tracing::info!(pr = pr_number, "digest posted"),
        Ok(Err(e)) => tracing::error!(pr = pr_number, error = %e, "digest failed"),
        Err(_) => tracing::error!(pr = pr_number, "digest timed out"),
    }
}

async fn spawn_impact(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    match timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()?;
        let auth = format!("Bearer {token}");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth)?,
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(USER_AGENT),
        );
        let url = format!(
            "https://api.github.com/repos/{}/pulls/{pr_number}/files?per_page=100",
            ctx.repo_full_name
        );
        let files: Vec<serde_json::Value> = client
            .get(&url)
            .headers(headers)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .unwrap_or_default();
        let changed_paths: Vec<String> = files
            .iter()
            .filter_map(|f| f["filename"].as_str().map(str::to_string))
            .collect();
        let mut parsed = Vec::new();
        for f in &files {
            let name = f["filename"].as_str().unwrap_or("");
            let patch = f["patch"].as_str().unwrap_or("");
            if name.is_empty() || patch.is_empty() {
                continue;
            }
            // Reconstruct approximate file content from added lines for import graph.
            let mut content = String::new();
            for line in patch.lines() {
                if let Some(rest) = line.strip_prefix('+').filter(|l| !l.starts_with("++")) {
                    content.push_str(rest);
                    content.push('\n');
                } else if !line.starts_with('-') && !line.starts_with('\\') && !line.starts_with('@')
                {
                    content.push_str(line);
                    content.push('\n');
                }
            }
            if let Ok(p) = crate::parser::parse_file(name, &content) {
                parsed.push(p);
            }
        }
        let report = crate::bot::blast::estimate_blast_radius(&parsed, &changed_paths);
        let mut text = String::from("### Codasaurus impact\n\n");
        let card = crate::bot::blast::blast_markdown(&report);
        if card.is_empty() {
            text.push_str(
                "> **Low impact** — no high-fan-in or high-sensitivity path signals from PR imports.\n",
            );
        } else {
            text.push_str(&card);
        }
        text.push_str("\n---\n");
        text.push_str(&crate::bot::markdown::commands_details());
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    {
        Ok(Ok(())) => tracing::info!(pr = pr_number, "impact completed"),
        Ok(Err(e)) => tracing::error!(pr = pr_number, error = %e, "impact failed"),
        Err(_) => tracing::error!(pr = pr_number, "impact timed out"),
    }
}

async fn spawn_similar(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()?;
        let auth = format!("Bearer {token}");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth)?,
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(USER_AGENT),
        );
        let paths = fetch_changed_paths_list(&token, &ctx.repo_full_name, pr_number).await?;
        let related = crate::bot::related_prs::find_related_prs(
            &client,
            &headers,
            &ctx.repo_full_name,
            &paths,
            pr_number,
        )
        .await
        .unwrap_or_default();
        let text = if related.is_empty() {
            "### Codasaurus similar\n\n> No related PRs found for these paths.\n".into()
        } else {
            let mut body = String::from(
                "### Codasaurus similar\n\nPRs that recently touched the same paths:\n\n",
            );
            for r in &related {
                body.push_str(&format!("- {r}\n"));
            }
            body.push_str("\n<sub>Ranked by shared path history (budgeted).</sub>\n");
            body
        };
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
}

async fn fetch_changed_paths_list(
    token: &str,
    repo: &str,
    pr_number: i64,
) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100");
    let files: Vec<serde_json::Value> = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .unwrap_or_default();
    Ok(files
        .iter()
        .filter_map(|f| f["filename"].as_str().map(str::to_string))
        .collect())
}

async fn spawn_fix(
    ctx: WebhookContext,
    pr_number: i64,
    fingerprint: Option<String>,
    timeout_secs: u64,
) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let pool = bot_db_pool();
        let mut allowed = false;
        if let Some(pool) = pool {
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "allow_auto_fix").await {
                allowed = matches!(
                    v.to_ascii_lowercase().as_str(),
                    "true" | "1" | "yes" | "on"
                );
            }
            if let Ok(Some(repo)) =
                crate::db::repos::get_repo_by_full_name(pool, &ctx.repo_full_name).await
            {
                if let Some(cfg) = repo.config_json.as_deref() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(cfg) {
                        if let Some(b) = val.get("allow_auto_fix").and_then(|v| v.as_bool()) {
                            allowed = b;
                        }
                    }
                }
            }
        }
        if !allowed {
            post_issue_comment(
                &get_installation_token(&ctx.cfg, ctx.inst_id).await?,
                &ctx.repo_full_name,
                pr_number,
                "### Codasaurus fix\n\n> Auto-fix is disabled.\n\nEnable `allow_auto_fix` in Settings or repo `config_json`.",
            )
            .await?;
            return Ok(());
        }

        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let head_ref = pr["head"]["ref"].as_str().unwrap_or("").to_string();
        let head_sha = pr["head"]["sha"].as_str().unwrap_or("").to_string();
        if head_ref.is_empty() {
            anyhow::bail!("missing head ref");
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let auth = format!("Bearer {token}");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth)?,
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(USER_AGENT),
        );

        let files_url = format!(
            "https://api.github.com/repos/{}/pulls/{}/files?per_page=100",
            ctx.repo_full_name, pr_number
        );
        let files: Vec<serde_json::Value> = client
            .get(&files_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .unwrap_or_default();

        let mut config = crate::config::Config::load_for_bot(pool).await;
        // Prefer stale_api for codemods
        config.checks.stale_api = true;
        let mut parsed = Vec::new();
        for f in &files {
            let name = f["filename"].as_str().unwrap_or("");
            let patch = f["patch"].as_str().unwrap_or("");
            if name.is_empty() || patch.is_empty() {
                continue;
            }
            if let Ok(p) = crate::parser::parse_unified_diff(name, patch) {
                parsed.push(p);
            }
        }
        let findings = crate::detectors::run_all(&parsed, &config);
        let fp_filter = fingerprint.as_deref();
        let with_codemod: Vec<_> = findings
            .findings
            .into_iter()
            .filter(|f| {
                let has_code = f.codemod.as_ref().is_some_and(|c| !c.is_empty()) && f.line > 0;
                if !has_code || f.detector == "phantom-deps" {
                    return false;
                }
                if let Some(want) = fp_filter {
                    let full = f.fingerprint();
                    full.starts_with(want) || want.starts_with(&full[..want.len().min(full.len())])
                } else {
                    true
                }
            })
            .take(if fp_filter.is_some() { 1 } else { 8 })
            .collect();

        if with_codemod.is_empty() {
            let msg = if fp_filter.is_some() {
                "### Codasaurus fix\n\n> No applyable codemod matched that fingerprint.\n\nUse GitHub **Apply suggestion** on the finding, or `@codasaurus fix` without a fingerprint."
            } else {
                "### Codasaurus fix\n\n> No applyable codemods on this PR.\n\nEnable detectors that emit replacements (e.g. stale-api), then re-run review."
            };
            post_issue_comment(&token, &ctx.repo_full_name, pr_number, msg).await?;
            return Ok(());
        }

        let git_ref = if head_sha.is_empty() {
            head_ref.as_str()
        } else {
            head_sha.as_str()
        };
        let mut applied = Vec::new();
        let mut by_file: std::collections::BTreeMap<String, Vec<_>> =
            std::collections::BTreeMap::new();
        for f in with_codemod {
            by_file.entry(f.file.clone()).or_default().push(f);
        }

        for (path, findings) in by_file {
            let Some((content, sha)) = crate::bot::github_files::fetch_repo_file_with_sha(
                &client,
                &headers,
                &ctx.repo_full_name,
                &path,
                git_ref,
            )
            .await?
            else {
                continue;
            };
            let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
            let mut changed = false;
            for f in findings {
                let Some(codemod) = f.codemod.as_deref() else {
                    continue;
                };
                let idx = f.line.saturating_sub(1);
                if idx < lines.len() {
                    lines[idx] = codemod.trim_end().to_string();
                    changed = true;
                    let short: String = f.fingerprint().chars().take(12).collect();
                    applied.push(format!("{path}:{} (`{short}`)", f.line));
                }
            }
            if !changed {
                continue;
            }
            let new_content = if content.ends_with('\n') {
                format!("{}\n", lines.join("\n"))
            } else {
                lines.join("\n")
            };
            crate::bot::github_files::put_repo_file(
                &client,
                &headers,
                &ctx.repo_full_name,
                &path,
                &head_ref,
                &new_content,
                &sha,
                &format!("chore: apply Codasaurus fix on {path}"),
            )
            .await?;
        }

        let text = if applied.is_empty() {
            "### Codasaurus fix\n\n> Could not apply codemods (file fetch/update failed).\n".into()
        } else {
            format!(
                "### Codasaurus fix\n\nApplied **{}** change(s) on `{head_ref}`:\n\n{}\n\n<sub>Review the commit before merging.</sub>",
                applied.len(),
                applied.iter().map(|s| format!("- `{s}`")).collect::<Vec<_>>().join("\n")
            )
        };
        post_issue_comment(&token, &ctx.repo_full_name, pr_number, &text).await
    })
    .await;
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
        assert!(matches!(
            parse_bot_command("@codasaurus update_changelog"),
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

    #[test]
    fn parses_similar_and_fix() {
        assert!(matches!(
            parse_bot_command("@codasaurus similar"),
            Some(BotCommand::Similar)
        ));
        assert!(matches!(
            parse_bot_command("@codasaurus impact"),
            Some(BotCommand::Impact)
        ));
        assert!(matches!(
            parse_bot_command("@codasaurus fix"),
            Some(BotCommand::Fix(None))
        ));
        match parse_bot_command("@codasaurus fix abcdef012345") {
            Some(BotCommand::Fix(Some(fp))) => assert!(fp.starts_with("abcdef")),
            other => panic!("expected Fix(Some), got {other:?}"),
        }
    }
}
