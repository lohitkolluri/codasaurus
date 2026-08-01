use axum::{http::StatusCode, Json};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::timeout;

mod auth;
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
        Ok(r) => r.rows_affected() == 0,
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

                if repo_full_name != "unknown" {
                    ensure_repo_exists(&repo_full_name, inst_id, &repo_val).await;
                }

                let lock = pr_lock(&repo_full_name, pr_number).await;
                let _guard = lock.lock().await;

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
            });
        }
    } else if event == "issue_comment" && payload.action == "created" {
        let comment_body = payload
            .comment
            .as_ref()
            .and_then(|c| c.get("body").and_then(|b| b.as_str()))
            .unwrap_or("");
        let is_review_cmd = comment_body.contains("@codasaurus review")
            || comment_body.contains("@codasaurus full review")
            || comment_body.contains("@codasaurus-bot review")
            || comment_body.contains("@codasaurus-bot full review");
        let is_ignore_cmd = comment_body.contains("@codasaurus-bot ignore")
            || comment_body.contains("@codasaurus ignore")
            || comment_body.contains("@codasaurus-bot dismiss")
            || comment_body.contains("@codasaurus dismiss");

        if (is_review_cmd || is_ignore_cmd) && author_can_command(&payload) {
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
                if is_review_cmd {
                    let timeout_secs = runtime.review_timeout_secs;
                    tokio::spawn(async move {
                        spawn_review(ctx, pr_number, timeout_secs).await;
                    });
                } else {
                    let fingerprint = extract_ignore_fingerprint(comment_body);
                    tokio::spawn(async move {
                        spawn_ignore_comment(ctx, pr_number, fingerprint).await;
                    });
                }
            }
        } else if is_review_cmd || is_ignore_cmd {
            tracing::info!("ignoring command from unauthorized commenter");
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

fn extract_ignore_fingerprint(body: &str) -> Option<String> {
    // `@codasaurus ignore <fingerprint>` or `@codasaurus-bot dismiss <fingerprint>`
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
    }
    None
}

async fn spawn_review(ctx: WebhookContext, pr_number: i64, timeout_secs: u64) {
    let lock = pr_lock(&ctx.repo_full_name, pr_number).await;
    let _guard = lock.lock().await;

    match timeout(Duration::from_secs(timeout_secs), async move {
        let token = get_installation_token(&ctx.cfg, ctx.inst_id).await?;
        if pr_number <= 0 || ctx.repo_full_name == "unknown" {
            anyhow::bail!("invalid repository or PR number");
        }
        let pr_data = fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await?;
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
                    "✅ Dismissed finding `{fp}`. Matching findings will be filtered on future reviews."
                )
            } else {
                format!(
                    "⚠️ Could not persist dismissal for `{fp}` (database unavailable)."
                )
            }
        } else {
            "To dismiss a finding, reply with `@codasaurus ignore <fingerprint>` \
             (copy the fingerprint from the finding details)."
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
