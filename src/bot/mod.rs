use axum::{http::StatusCode, Json};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) mod agent_mode;
mod auth;
pub(crate) mod blast;
mod codeowners;
mod commands;
pub(crate) mod concern;
pub(crate) mod dep_delta;
mod github_extra;
pub(crate) mod github_files;
pub(crate) mod grounding;
mod issue_assessment;
pub(crate) mod maintenance;
pub(crate) mod markdown;
pub(crate) mod offline;
mod policy;
pub(crate) mod provenance;
mod quality;
pub(crate) use quality::{apply_signal_budget, SignalBudget};
pub mod queue;
mod reactions;
pub(crate) mod related_prs;
pub(crate) mod repo_context;
mod review;
mod threads;
pub(crate) use review::{github_api_headers, next_github_link, GITHUB_CLIENT};
pub(crate) mod strictness;
pub(crate) mod title_fix;
mod verify;
mod worker;
pub use worker::start_review_worker;

use crate::bot_runtime::BotRuntimeConfig;
use crate::db::{self, models::RepoCreate};

pub(crate) static USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));

/// Per-PR locks to prevent concurrent duplicate reviews.
static PR_LOCKS: std::sync::LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) async fn pr_lock(repo: &str, pr_number: i64) -> Arc<Mutex<()>> {
    let key = format!("{repo}:{pr_number}");
    let mut map = PR_LOCKS.lock().await;
    map.entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Drop idle PR lock entries so the map does not grow forever on long-lived serve.
pub(crate) async fn prune_pr_lock(repo: &str, pr_number: i64) {
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
    let result = crate::db::db_execute!(
        pool,
        "INSERT INTO webhook_deliveries (delivery_id) VALUES (?) ON CONFLICT(delivery_id) DO NOTHING",
        delivery_id
    );

    match result {
        Ok(r) => {
            if r > 0 {
                let _ = crate::db::db_execute!(
                    pool,
                    "DELETE FROM webhook_deliveries WHERE received_at < NOW() - INTERVAL '14 days'"
                );
            }
            r == 0
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
pub(crate) struct WebhookContext {
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
static WORKER_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_bot_config(config: BotConfig) {
    *BOT_CONFIG.write().expect("BOT_CONFIG lock poisoned") = Some(config);
}

/// Clear in-memory GitHub App credentials (e.g. after dashboard delete).
pub fn clear_bot_config() {
    *BOT_CONFIG.write().expect("BOT_CONFIG lock poisoned") = None;
}

/// Snapshot of the live bot config (reloaded after wizard / settings updates).
pub(crate) fn current_bot_config() -> Option<BotConfig> {
    BOT_CONFIG.read().expect("BOT_CONFIG lock poisoned").clone()
}

/// Start durable queue workers once. Safe to call after wizard GitHub setup.
pub fn ensure_review_worker(pool: crate::db::DbPool) {
    if WORKER_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    worker::start_review_worker(pool);
}

pub fn set_config_pool(pool: crate::db::DbPool) {
    let _ = CONFIG_POOL.set(pool);
}

pub(crate) fn bot_db_pool() -> Option<&'static crate::db::DbPool> {
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
        .filter(|k| !k.trim().is_empty())
        .or_else(crate::github_jwt::resolve_private_key_from_env)?;
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
    /// `reaction` webhook event payload
    reaction: Option<serde_json::Value>,
    /// `pull_request_review_thread` webhook event payload
    thread: Option<serde_json::Value>,
    /// Actor who triggered the event (reactions, etc.)
    sender: Option<serde_json::Value>,
    /// Sent in `installation.created` event
    repositories: Option<Vec<serde_json::Value>>,
    /// Sent in `installation_repositories.added` event
    #[serde(rename = "repositories_added")]
    repositories_added: Option<Vec<serde_json::Value>>,
}

/// Comment author association / identity from GitHub payload.
fn comment_author_login(payload: &WebhookPayload) -> &str {
    payload
        .comment
        .as_ref()
        .and_then(|c| c.pointer("/user/login"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn comment_author_association(payload: &WebhookPayload) -> &str {
    payload
        .comment
        .as_ref()
        .and_then(|c| c.get("author_association"))
        .and_then(|a| a.as_str())
        .unwrap_or("")
}

/// True when the comment was written by a GitHub App / bot (including ourselves).
///
/// Our own review comments mention `@codasaurus …` in the Commands footer; without
/// this filter, `issue_comment.created` re-parses those as commands and the bot
/// ACL-denies itself (`association: NONE`).
fn comment_from_bot(payload: &WebhookPayload) -> bool {
    let comment = match payload.comment.as_ref() {
        Some(c) => c,
        None => return false,
    };
    if comment
        .pointer("/user/type")
        .and_then(|v| v.as_str())
        .is_some_and(|t| t.eq_ignore_ascii_case("Bot"))
    {
        return true;
    }
    let login = comment
        .pointer("/user/login")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if login.is_empty() {
        return false;
    }
    let login_l = login.to_ascii_lowercase();
    if login_l.ends_with("[bot]") {
        return true;
    }
    if let Ok(slug) = std::env::var("GITHUB_APP_SLUG") {
        let slug = slug.trim().to_ascii_lowercase();
        if !slug.is_empty() && (login_l == slug || login_l == format!("{slug}[bot]")) {
            return true;
        }
    }
    false
}

/// Who may run `@codasaurus …` commands on a PR.
///
/// GitHub associations vary (personal repos vs orgs vs forks). Allow owners,
/// org members, collaborators, prior contributors, and the PR author / repo owner
/// by login — not only OWNER|MEMBER|COLLABORATOR.
fn author_can_command(payload: &WebhookPayload) -> bool {
    let association = comment_author_association(payload);
    if matches!(
        association,
        "OWNER" | "MEMBER" | "COLLABORATOR" | "CONTRIBUTOR"
    ) {
        return true;
    }

    let commenter = comment_author_login(payload);
    if commenter.is_empty() {
        return false;
    }

    let repo_owner = payload
        .repo
        .as_ref()
        .and_then(|r| r.pointer("/owner/login"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if commenter.eq_ignore_ascii_case(repo_owner) {
        return true;
    }

    // Issue (PR) author may command on their own PR.
    let pr_author = payload
        .issue
        .as_ref()
        .and_then(|i| i.pointer("/user/login"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    commenter.eq_ignore_ascii_case(pr_author)
}

/// Who may dismiss findings via emoji reactions (same trust bar as slash commands).
fn reactor_can_dismiss(payload: &WebhookPayload) -> bool {
    let reaction_assoc = payload
        .reaction
        .as_ref()
        .and_then(|r| r.get("author_association"))
        .and_then(|a| a.as_str())
        .unwrap_or("");
    if matches!(
        reaction_assoc,
        "OWNER" | "MEMBER" | "COLLABORATOR" | "CONTRIBUTOR"
    ) {
        return true;
    }

    let reactor = payload
        .reaction
        .as_ref()
        .and_then(|r| r.pointer("/user/login"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            payload
                .sender
                .as_ref()
                .and_then(|s| s.get("login"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("");
    if reactor.is_empty() {
        return false;
    }

    let repo_owner = payload
        .repo
        .as_ref()
        .and_then(|r| r.pointer("/owner/login"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if reactor.eq_ignore_ascii_case(repo_owner) {
        return true;
    }

    let pr_author = payload
        .issue
        .as_ref()
        .and_then(|i| i.pointer("/user/login"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            payload
                .pull_request
                .as_ref()
                .and_then(|pr| pr.pointer("/user/login"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("");
    reactor.eq_ignore_ascii_case(pr_author)
}

/// Who may dismiss findings by resolving a review thread (same trust bar as reactions).
fn thread_resolver_can_dismiss(payload: &WebhookPayload) -> bool {
    let assoc = payload
        .thread
        .as_ref()
        .and_then(|t| t.pointer("/comments/0/author_association"))
        .and_then(|a| a.as_str())
        .unwrap_or("");
    if matches!(assoc, "OWNER" | "MEMBER" | "COLLABORATOR" | "CONTRIBUTOR") {
        return true;
    }

    let resolver = payload
        .thread
        .as_ref()
        .and_then(|t| t.pointer("/comments/0/user/login"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            payload
                .sender
                .as_ref()
                .and_then(|s| s.get("login"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("");
    if resolver.is_empty() {
        return false;
    }

    let repo_owner = payload
        .repo
        .as_ref()
        .and_then(|r| r.pointer("/owner/login"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if resolver.eq_ignore_ascii_case(repo_owner) {
        return true;
    }

    let pr_author = payload
        .pull_request
        .as_ref()
        .and_then(|pr| pr.pointer("/user/login"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    resolver.eq_ignore_ascii_case(pr_author)
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
            let pr_number = pr.as_ref().and_then(|p| p["number"].as_i64()).unwrap_or(0);
            let cfg = config.clone();
            let delivery = delivery_id.to_string();
            let timeout_secs = runtime.review_timeout_secs;
            let action = payload.action.clone();
            let head_sha = pr
                .as_ref()
                .and_then(|p| p["head"]["sha"].as_str())
                .unwrap_or("")
                .to_string();
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

                // Opt-in: do not enqueue for inactive repos (avoids silent mark_done no-ops).
                if let Some(pool) = bot_db_pool() {
                    if let Ok(Some(repo)) =
                        db::repos::get_repo_by_full_name(pool, &repo_full_name).await
                    {
                        if !repo.active {
                            tracing::info!(
                                repo = %repo_full_name,
                                "repo inactive; skipping enqueue (enable in dashboard)"
                            );
                            return;
                        }
                    }
                }

                // Durable queue: persist job; background worker runs the review.
                if let Some(pool) = bot_db_pool() {
                    match queue::enqueue(
                        pool,
                        &repo_full_name,
                        pr_number,
                        &head_sha,
                        inst_id,
                        &action,
                    )
                    .await
                    {
                        Ok(job_id) => {
                            tracing::info!(job_id, "enqueued review job");
                            worker::QUEUE_NOTIFY.notify_waiters();
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "queue enqueue failed; running inline");
                            worker::run_webhook_review_inline(
                                cfg,
                                repo_full_name,
                                pr_number,
                                pr,
                                inst_id,
                                action,
                                head_sha,
                                timeout_secs,
                            )
                            .await;
                        }
                    }
                } else {
                    worker::run_webhook_review_inline(
                        cfg,
                        repo_full_name,
                        pr_number,
                        pr,
                        inst_id,
                        action,
                        head_sha,
                        timeout_secs,
                    )
                    .await;
                }
            });
        }
    } else if event == "issue_comment" && payload.action == "created" {
        // Ignore our own (and other bots') comments — review footers mention
        // `@codasaurus help` / command names and must not re-trigger ACL denials.
        if comment_from_bot(&payload) {
            return Ok(Json(
                serde_json::json!({"status": "ok", "ignored": "bot_comment"}),
            ));
        }
        let comment_body = payload
            .comment
            .as_ref()
            .and_then(|c| c.get("body").and_then(|b| b.as_str()))
            .unwrap_or("");
        let cmd = commands::parse_bot_command(comment_body);

        if let Some(cmd) = cmd {
            let is_pr = payload
                .issue
                .as_ref()
                .and_then(|i| i.get("pull_request"))
                .is_some();
            let pr_number = payload
                .issue
                .as_ref()
                .and_then(|i| i["number"].as_i64())
                .unwrap_or(0);
            if !author_can_command(&payload) {
                let association = comment_author_association(&payload).to_string();
                let commenter = comment_author_login(&payload).to_string();
                tracing::info!(
                    %association,
                    %commenter,
                    pr = pr_number,
                    "ignoring command from unauthorized commenter"
                );
                if is_pr && pr_number > 0 {
                    let ctx = WebhookContext::from_payload(config.clone(), &payload);
                    let notice = format!(
                        "### Codasaurus\n\n\
                         Commands are limited to the **repo owner**, org **members**, \
                         **collaborators**, prior **contributors**, and the **PR author**.\n\n\
                         Commenter `{commenter}` has association `{association}`.\n\n\
                         <sub>If you own this repo, check that you commented from the same \
                         GitHub account that owns it.</sub>"
                    );
                    tokio::spawn(async move {
                        commands::notify_command_denied(ctx, pr_number, notice).await;
                    });
                }
            } else if is_pr && pr_number > 0 {
                let ctx = WebhookContext::from_payload(config.clone(), &payload);
                let timeout_secs = runtime.review_timeout_secs;
                tokio::spawn(async move {
                    commands::handle_bot_command(ctx, pr_number, cmd, timeout_secs).await;
                });
            }
        }
    } else if event == "reaction" && payload.action == "created" {
        let content = payload
            .reaction
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        let comment_body = payload
            .comment
            .as_ref()
            .and_then(|c| c.get("body"))
            .and_then(|b| b.as_str())
            .unwrap_or("");
        let repo_full_name = payload
            .repo
            .as_ref()
            .and_then(|r| r["full_name"].as_str())
            .unwrap_or("")
            .to_string();
        if !content.is_empty() && !comment_body.is_empty() && !repo_full_name.is_empty() {
            let content = content.to_string();
            let comment_body = comment_body.to_string();
            let allowed = reactor_can_dismiss(&payload);
            tokio::spawn(async move {
                if let Some(pool) = bot_db_pool() {
                    if let Err(e) = reactions::handle_reaction_event(
                        pool,
                        "created",
                        &content,
                        &comment_body,
                        &repo_full_name,
                        allowed,
                    )
                    .await
                    {
                        tracing::warn!(error = %e, "reaction learning failed");
                    }
                }
            });
        }
    } else if event == "pull_request_review_thread"
        && matches!(payload.action.as_str(), "resolved" | "unresolved")
    {
        let repo_full_name = payload
            .repo
            .as_ref()
            .and_then(|r| r["full_name"].as_str())
            .unwrap_or("")
            .to_string();
        if !repo_full_name.is_empty() {
            if let Some(thread) = payload.thread.clone() {
                let action = payload.action.clone();
                let allowed = thread_resolver_can_dismiss(&payload);
                tokio::spawn(async move {
                    if let Some(pool) = bot_db_pool() {
                        if let Err(e) = threads::handle_thread_event(
                            pool,
                            &action,
                            &thread,
                            &repo_full_name,
                            allowed,
                        )
                        .await
                        {
                            tracing::warn!(error = %e, "review thread learning failed");
                        }
                    }
                });
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

/// Global concurrency limit for reviews (org-scale safety).
pub(crate) static REVIEW_PERMITS: std::sync::LazyLock<tokio::sync::Semaphore> =
    std::sync::LazyLock::new(|| {
        let default = if std::env::var_os("CODASAURUS_FREE_TIER").is_some()
            || std::env::var_os("RENDER").is_some()
            || std::env::var_os("RENDER_SERVICE_ID").is_some()
        {
            1usize
        } else {
            4usize
        };
        let n = std::env::var("CODASAURUS_MAX_CONCURRENT_REVIEWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default);
        tokio::sync::Semaphore::new(n.max(1))
    });

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

        if let Err(e) = db::repos::create_repo_from_installation(
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
    match crate::db::db_execute!(
        pool,
        "UPDATE repos SET active = ? WHERE installation_id = ?",
        false,
        inst_id
    ) {
        Ok(r) => {
            tracing::info!(deactivated = r, installation = inst_id, "deactivated repos");
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

        if let Err(e) = db::repos::create_repo_from_installation(
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

#[cfg(test)]
mod author_acl_tests {
    use super::*;

    fn payload(assoc: &str, commenter: &str, repo_owner: &str, pr_author: &str) -> WebhookPayload {
        WebhookPayload {
            action: "created".into(),
            comment: Some(serde_json::json!({
                "author_association": assoc,
                "user": { "login": commenter },
                "body": "@codasaurus help"
            })),
            repo: Some(serde_json::json!({
                "full_name": format!("{repo_owner}/demo"),
                "owner": { "login": repo_owner }
            })),
            issue: Some(serde_json::json!({
                "number": 1,
                "user": { "login": pr_author },
                "pull_request": {}
            })),
            pull_request: None,
            installation: None,
            reaction: None,
            thread: None,
            sender: None,
            repositories: None,
            repositories_added: None,
        }
    }

    #[test]
    fn ignores_bot_commenters() {
        let mut p = payload("NONE", "codasaurus-e0a6[bot]", "alice", "bob");
        if let Some(c) = p.comment.as_mut() {
            c["user"]["type"] = serde_json::json!("Bot");
        }
        assert!(comment_from_bot(&p));
        assert!(comment_from_bot(&payload(
            "NONE",
            "dependabot[bot]",
            "alice",
            "bob"
        )));
        assert!(!comment_from_bot(&payload(
            "OWNER", "alice", "alice", "bob"
        )));
    }

    #[test]
    fn allows_owner_association() {
        assert!(author_can_command(&payload(
            "OWNER", "alice", "alice", "bob"
        )));
    }

    #[test]
    fn allows_repo_owner_login_even_if_association_none() {
        assert!(author_can_command(&payload(
            "NONE", "alice", "alice", "carol"
        )));
    }

    #[test]
    fn allows_pr_author() {
        assert!(author_can_command(&payload(
            "FIRST_TIME_CONTRIBUTOR",
            "bob",
            "org",
            "bob"
        )));
    }

    #[test]
    fn rejects_unrelated_none() {
        assert!(!author_can_command(&payload("NONE", "eve", "alice", "bob")));
    }

    fn thread_payload(
        assoc: &str,
        resolver: &str,
        repo_owner: &str,
        pr_author: &str,
    ) -> WebhookPayload {
        let mut p = payload(assoc, resolver, repo_owner, pr_author);
        p.thread = Some(serde_json::json!({
            "node_id": "PRRT_x",
            "comments": [{
                "author_association": assoc,
                "user": { "login": resolver },
                "body": "**Secrets** · `blocking`\n<sub>`fingerprint: abcdef012345`</sub>"
            }]
        }));
        p.pull_request = Some(serde_json::json!({
            "user": { "login": pr_author }
        }));
        p
    }

    #[test]
    fn thread_owner_association_allows_dismiss() {
        assert!(thread_resolver_can_dismiss(&thread_payload(
            "OWNER", "alice", "alice", "bob"
        )));
    }

    #[test]
    fn thread_repo_owner_login_allows_even_if_none() {
        assert!(thread_resolver_can_dismiss(&thread_payload(
            "NONE", "alice", "alice", "carol"
        )));
    }

    #[test]
    fn thread_pr_author_allows() {
        assert!(thread_resolver_can_dismiss(&thread_payload(
            "FIRST_TIME_CONTRIBUTOR",
            "bob",
            "org",
            "bob"
        )));
    }

    #[test]
    fn thread_unrelated_none_rejected() {
        assert!(!thread_resolver_can_dismiss(&thread_payload(
            "NONE", "eve", "alice", "bob"
        )));
    }
}
