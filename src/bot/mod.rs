use axum::{http::StatusCode, Json};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::timeout;

mod auth;
mod codeowners;
mod github_files;
pub(crate) mod markdown;
pub(crate) mod repo_context;
mod review;
mod verify;

use self::auth::get_installation_token;
use self::review::{fetch_pull_request, review_pr};

use crate::bot_runtime::BotRuntimeConfig;
use crate::db::{self, models::RepoCreate};
use crate::learning::store::LearningStore;

static USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));

/// Per-PR locks to prevent concurrent duplicate reviews.
static PR_LOCKS: std::sync::LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

async fn pr_lock(repo: &str, pr_number: i64) -> Arc<Mutex<()>> {
    let key = format!("{repo}:{pr_number}");
    let mut map = PR_LOCKS.lock().await;
    map.entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Drop idle PR lock entries so the map does not grow forever on long-lived serve.
async fn prune_pr_lock(repo: &str, pr_number: i64) {
    let key = format!("{repo}:{pr_number}");
    let mut map = PR_LOCKS.lock().await;
    if let Some(lock) = map.get(&key) {
        // Only this map entry holds the Arc (no active holders).
        if Arc::strong_count(lock) == 1 {
            map.remove(&key);
        }
    }
}

/// Persist delivery ID; returns true if this delivery was already seen.
async fn is_duplicate_delivery(delivery_id: &str) -> bool {
    let Some(pool) = CONFIG_POOL.get() else {
        // No DB — fall back to in-memory only for this process
        return false;
    };
    let result = sqlx::query(
        "INSERT OR IGNORE INTO webhook_deliveries (delivery_id) VALUES (?)",
    )
    .bind(delivery_id)
    .execute(&pool.0)
    .await;

    match result {
        Ok(r) => {
            // Opportunistic prune so the dedup table cannot grow forever.
            if r.rows_affected() > 0 {
                let _ = sqlx::query(
                    "DELETE FROM webhook_deliveries WHERE received_at < datetime('now', '-14 days')",
                )
                .execute(&pool.0)
                .await;
            }
            r.rows_affected() == 0
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to record webhook delivery");
            false
        }
    }
}

#[derive(Clone)]
pub struct BotConfig {
    pub app_id: String,
    pub private_key: String,
    pub webhook_secret: String,
    pub host: String,
    pub port: u16,
}

#[derive(Clone)]
struct WebhookContext {
    cfg: BotConfig,
    inst_id: Option<i64>,
    repo_full_name: String,
}

impl WebhookContext {
    fn from_payload(cfg: BotConfig, payload: &WebhookPayload) -> Self {
        Self {
            inst_id: payload.installation.as_ref().map(|i| i.id),
            repo_full_name: payload
                .repo
                .as_ref()
                .and_then(|r| r["full_name"].as_str())
                .unwrap_or("unknown")
                .to_string(),
            cfg,
        }
    }
}

static BOT_CONFIG: std::sync::RwLock<Option<BotConfig>> = std::sync::RwLock::new(None);
pub(crate) static CONFIG_POOL: std::sync::OnceLock<crate::db::DbPool> = std::sync::OnceLock::new();

pub fn set_bot_config(config: BotConfig) {
    *BOT_CONFIG.write().expect("BOT_CONFIG lock poisoned") = Some(config);
}

pub fn set_config_pool(pool: crate::db::DbPool) {
    let _ = CONFIG_POOL.set(pool);
}

fn bot_db_pool() -> Option<&'static crate::db::DbPool> {
    CONFIG_POOL.get()
}

async fn reload_bot_config() -> Option<BotConfig> {
    let pool = CONFIG_POOL.get()?;
    let app_id = db::config::get_config(pool, "github_app_id")
        .await
        .ok()
        .flatten()?;
    let private_key = db::config::get_config(pool, "github_private_key")
        .await
        .ok()
        .flatten()
        .or_else(|| {
            std::env::var("GITHUB_APP_PRIVATE_KEY_B64")
                .ok()
                .and_then(|b64| {
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64)
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                })
        })?;
    let webhook_secret = db::config::get_config(pool, "github_webhook_secret")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("GITHUB_WEBHOOK_SECRET").ok())
        .unwrap_or_default();
    Some(BotConfig {
        app_id,
        private_key,
        webhook_secret,
        host: "0.0.0.0".into(),
        port: 3000,
    })
}

#[derive(Deserialize)]
struct InstallationInfo {
    id: i64,
}

#[derive(Deserialize)]
pub(crate) struct WebhookPayload {
    #[serde(rename = "action")]
    action: String,
    #[serde(rename = "pull_request")]
    pull_request: Option<serde_json::Value>,
    #[serde(rename = "repository")]
    repo: Option<serde_json::Value>,
    installation: Option<InstallationInfo>,
    #[serde(rename = "comment")]
    comment: Option<serde_json::Value>,
    #[serde(rename = "issue")]
    issue: Option<serde_json::Value>,
    /// Sent in `installation.created` event
    repositories: Option<Vec<serde_json::Value>>,
    /// Sent in `installation_repositories.added` event
    #[serde(rename = "repositories_added")]
    repositories_added: Option<Vec<serde_json::Value>>,
}

/// Comment author association from GitHub payload.
fn author_can_command(payload: &WebhookPayload) -> bool {
    let association = payload
        .comment
        .as_ref()
        .and_then(|c| c.get("author_association"))
        .and_then(|a| a.as_str())
        .unwrap_or("");
    matches!(
        association,
        "OWNER" | "MEMBER" | "COLLABORATOR" | "CONTRIBUTOR"
    )
}

pub(crate) async fn handle_webhook(
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut config = BOT_CONFIG.read().expect("BOT_CONFIG lock poisoned").clone();

    if config
        .as_ref()
        .map(|c| c.webhook_secret.is_empty())
        .unwrap_or(true)
    {
        if let Some(reloaded) = reload_bot_config().await {
            if !reloaded.webhook_secret.is_empty() {
                set_bot_config(reloaded.clone());
                config = Some(reloaded);
            }
        }
    }

    let config = config.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let sig = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify::verify_signature(&config.webhook_secret, &body, sig) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if delivery_id.is_empty() {
        tracing::warn!("rejecting webhook without X-GitHub-Delivery");
        return Err(StatusCode::BAD_REQUEST);
    }
    if is_duplicate_delivery(delivery_id).await {
        return Ok(Json(serde_json::json!({"status": "ok", "duplicate": true})));
    }

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let payload: WebhookPayload =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let runtime = BotRuntimeConfig::default();

    if event == "pull_request"
        && matches!(
            payload.action.as_str(),
            "opened" | "reopened" | "synchronize" | "ready_for_review"
        )
    {
        if payload.pull_request.is_some() {
            let repo_val = payload.repo.clone();
            let repo_full_name = payload
                .repo
                .as_ref()
                .and_then(|r| r["full_name"].as_str())
                .unwrap_or("unknown")
                .to_string();
            let inst_id = payload.installation.as_ref().map(|i| i.id);
            let pr = payload.pull_request.as_ref().cloned();
            let pr_number = pr
                .as_ref()
                .and_then(|p| p["number"].as_i64())
                .unwrap_or(0);
            let cfg = config.clone();
            let delivery = delivery_id.to_string();
            let timeout_secs = runtime.review_timeout_secs;
            tokio::spawn(async move {
                let span = tracing::info_span!(
                    "review_pr_webhook",
                    delivery_id = %delivery,
                    repo = %repo_full_name,
                    pr = pr_number
                );
                let _enter = span.enter();

                let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
                    tracing::error!("review semaphore closed");
                    return;
                };

                if repo_full_name != "unknown" {
                    ensure_repo_exists(&repo_full_name, inst_id, &repo_val).await;
                }

                let lock = pr_lock(&repo_full_name, pr_number).await;
                let _guard = lock.lock().await;

                let repo_for_prune = repo_full_name.clone();
                match timeout(Duration::from_secs(timeout_secs), async move {
                    let token = get_installation_token(&cfg, inst_id).await?;
                    let wrapped = WebhookPayload {
                        action: String::new(),
                        pull_request: pr,
                        repo: None,
                        installation: None,
                        comment: None,
                        issue: None,
                        repositories: None,
                        repositories_added: None,
                    };
                    review_pr(&token, &repo_full_name, &wrapped).await
                })
                .await
                {
                    Ok(Ok(())) => tracing::info!("review completed"),
                    Ok(Err(e)) => tracing::error!(error = %e, "review failed"),
                    Err(_) => tracing::error!("review timed out"),
                }

                drop(_guard);
                prune_pr_lock(&repo_for_prune, pr_number).await;
            });
        }
    } else if event == "issue_comment" && payload.action == "created" {
        let comment_body = payload
            .comment
            .as_ref()
            .and_then(|c| c.get("body").and_then(|b| b.as_str()))
            .unwrap_or("");
        let cmd = parse_bot_command(comment_body);

        if let Some(cmd) = cmd {
            if !author_can_command(&payload) {
                tracing::info!("ignoring command from unauthorized commenter");
            } else {
                let is_pr = payload
                    .issue
                    .as_ref()
                    .and_then(|i| i.get("pull_request"))
                    .is_some();
                if is_pr {
                    let pr_number = payload
                        .issue
                        .as_ref()
                        .and_then(|i| i["number"].as_i64())
                        .unwrap_or(0);
                    let ctx = WebhookContext::from_payload(config.clone(), &payload);
                    let timeout_secs = runtime.review_timeout_secs;
                    tokio::spawn(async move {
                        handle_bot_command(ctx, pr_number, cmd, timeout_secs).await;
                    });
                }
            }
        }
    } else if event == "installation" && payload.action == "created" {
        tokio::spawn(handle_installation_created(
            payload.installation,
            payload.repositories.unwrap_or_default(),
        ));
    } else if event == "installation" && payload.action == "deleted" {
        tokio::spawn(handle_installation_deleted(payload.installation));
    } else if event == "installation_repositories" && payload.action == "added" {
        tokio::spawn(handle_repos_added(
            payload.installation,
            payload.repositories_added.unwrap_or_default(),
        ));
    }
    Ok(Json(serde_json::json!({"status": "ok"})))
}

#[derive(Debug, Clone)]
enum BotCommand {
    Review,
    Describe,
    Improve,
    Ask(String),
    Ignore(Option<String>),
    Help,
}

fn parse_bot_command(body: &str) -> Option<BotCommand> {
    let lower = body.to_ascii_lowercase();
    let mentions = ["@codasaurus", "@codasaurus-bot"];
    if !mentions.iter().any(|m| lower.contains(m)) {
        return None;
    }
    if lower.contains(" help") || lower.ends_with("help") {
        // avoid matching "helpful" — require word boundary-ish
        if lower.contains("@codasaurus help")
            || lower.contains("@codasaurus-bot help")
            || lower.contains("@codasaurus-bot help")
        {
            return Some(BotCommand::Help);
        }
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

/// Global concurrency limit for reviews (org-scale safety).
static REVIEW_PERMITS: std::sync::LazyLock<tokio::sync::Semaphore> =
    std::sync::LazyLock::new(|| {
        let n = std::env::var("CODASAURUS_MAX_CONCURRENT_REVIEWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4usize);
        tokio::sync::Semaphore::new(n.max(1))
    });

async fn handle_bot_command(
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
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("");
        let body = pr["body"].as_str().unwrap_or("");
        let pool = bot_db_pool();
        let text = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            match crate::llm::summarize_pr(title, body, "(describe walkthrough)", &llm).await {
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
    let _ = timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        let pr = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
        let title = pr["title"].as_str().unwrap_or("");
        let body = pr["body"].as_str().unwrap_or("");
        let pool = bot_db_pool();
        let answer = if let Some(llm) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            let findings_ctx = format!("PR title: {title}\n\nQuestion: {question}");
            crate::llm::summarize_pr(title, body, &findings_ctx, &llm)
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

async fn spawn_review(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let Ok(_permit) = REVIEW_PERMITS.acquire().await else {
        tracing::error!("review semaphore closed");
        return;
    };
    let repo_name = ctx.repo_full_name.clone();
    let lock = pr_lock(&repo_name, pr_number).await;
    let _guard = lock.lock().await;

    match timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        if pr_number <= 0 || ctx.repo_full_name == "unknown" {
            anyhow::bail!("invalid repository or PR number");
        }
        let pr_data = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
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
        Ok(Err(e)) => tracing::error!(pr = pr_number, error = %e, "comment-triggered review failed"),
        Err(_) => tracing::error!(pr = pr_number, "comment-triggered review timed out"),
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

async fn handle_installation_created(
    installation: Option<InstallationInfo>,
    repos: Vec<serde_json::Value>,
) {
    let inst_id = match installation {
        Some(i) => i.id,
        None => return,
    };
    let pool = match bot_db_pool() {
        Some(p) => p,
        None => {
            tracing::warn!("DB pool not available, skipping repo sync");
            return;
        }
    };
    for repo in &repos {
        let github_id = repo["id"].as_i64();
        let full_name = match repo["full_name"].as_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        let parts: Vec<&str> = full_name.splitn(2, '/').collect();
        if parts.len() < 2 {
            continue;
        }
        let owner = parts[0].to_string();
        let name = parts[1].to_string();
        let default_branch = repo["default_branch"].as_str().map(|s| s.to_string());
        let private = repo["private"].as_bool().unwrap_or(false);

        if let Err(e) = db::repos::create_repo(
            pool,
            &RepoCreate {
                github_id,
                full_name,
                owner,
                name,
                default_branch,
                installation_id: inst_id,
                private,
            },
        )
        .await
        {
            tracing::error!(error = %e, "failed to save repo");
        }
    }
    let count = repos.len();
    if let Some(p) = bot_db_pool() {
        crate::db::audit::log_event(
            p,
            "installation.created",
            None,
            Some("installation"),
            Some(inst_id),
        )
        .await;
    }
    tracing::info!(count, "synced repos from installation");
}

async fn handle_installation_deleted(installation: Option<InstallationInfo>) {
    let inst_id = match installation {
        Some(i) => i.id,
        None => return,
    };
    let pool = match bot_db_pool() {
        Some(p) => p,
        None => {
            tracing::warn!("Cannot deactivate repos: DB pool not available");
            return;
        }
    };
    match sqlx::query("UPDATE repos SET active = 0 WHERE installation_id = ?")
        .bind(inst_id)
        .execute(&pool.0)
        .await
    {
        Ok(r) => {
            tracing::info!(
                deactivated = r.rows_affected(),
                installation = inst_id,
                "deactivated repos"
            );
            crate::db::audit::log_event(
                pool,
                "installation.deleted",
                None,
                Some("installation"),
                Some(inst_id),
            )
            .await;
        }
        Err(e) => tracing::error!(error = %e, "failed to deactivate repos"),
    }
}

async fn handle_repos_added(installation: Option<InstallationInfo>, repos: Vec<serde_json::Value>) {
    let inst_id = match installation {
        Some(i) => i.id,
        None => return,
    };
    let pool = match bot_db_pool() {
        Some(p) => p,
        None => return,
    };
    for repo in &repos {
        let github_id = repo["id"].as_i64();
        let full_name = match repo["full_name"].as_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        let parts: Vec<&str> = full_name.splitn(2, '/').collect();
        if parts.len() < 2 {
            continue;
        }
        let owner = parts[0].to_string();
        let name = parts[1].to_string();
        let default_branch = repo["default_branch"].as_str().map(|s| s.to_string());
        let private = repo["private"].as_bool().unwrap_or(false);

        if let Err(e) = db::repos::create_repo(
            pool,
            &RepoCreate {
                github_id,
                full_name,
                owner,
                name,
                default_branch,
                installation_id: inst_id,
                private,
            },
        )
        .await
        {
            tracing::error!(error = %e, "failed to save repo");
        }
    }
    tracing::info!(count = repos.len(), "synced added repos");
}

/// Auto-register a repo when we receive a webhook event for it.
async fn ensure_repo_exists(
    full_name: &str,
    inst_id: Option<i64>,
    repo_val: &Option<serde_json::Value>,
) {
    let pool = match bot_db_pool() {
        Some(p) => p,
        None => return,
    };
    if let Ok(Some(_)) = db::repos::get_repo_by_full_name(pool, full_name).await {
        return;
    }
    let inst = inst_id.unwrap_or(0);
    let github_id = repo_val.as_ref().and_then(|r| r["id"].as_i64());
    let default_branch = repo_val
        .as_ref()
        .and_then(|r| r["default_branch"].as_str().map(String::from));
    let private = repo_val
        .as_ref()
        .and_then(|r| r["private"].as_bool())
        .unwrap_or(false);
    let parts: Vec<&str> = full_name.splitn(2, '/').collect();
    if parts.len() < 2 {
        return;
    }
    if let Err(e) = db::repos::create_repo(
        pool,
        &RepoCreate {
            github_id,
            full_name: full_name.to_string(),
            owner: parts[0].to_string(),
            name: parts[1].to_string(),
            default_branch,
            installation_id: inst,
            private,
        },
    )
    .await
    {
        tracing::error!(repo = full_name, error = %e, "failed to auto-register repo");
    }
}
