use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::time::timeout;

mod auth;
mod review;
mod verify;

use self::auth::get_installation_token;
use self::review::{fetch_pull_request, review_pr};

static USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));

/// Tracks recently processed webhook delivery IDs to prevent replay attacks.
/// Cleared every hour to bound memory use.
struct DeliveryTracker {
    seen: HashSet<String>,
    last_cleanup: Instant,
}

impl DeliveryTracker {
    fn new() -> Self {
        Self {
            seen: HashSet::new(),
            last_cleanup: Instant::now(),
        }
    }

    /// Returns `true` if this delivery ID has already been processed.
    fn is_duplicate(&mut self, id: &str) -> bool {
        if self.last_cleanup.elapsed() > Duration::from_secs(3600) {
            self.seen.clear();
            self.last_cleanup = Instant::now();
        }
        !self.seen.insert(id.to_string())
    }
}

static DELIVERY_TRACKER: std::sync::LazyLock<Mutex<DeliveryTracker>> =
    std::sync::LazyLock::new(|| Mutex::new(DeliveryTracker::new()));

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

/// Build the webhook-only router for mounting under `/webhook` in the unified server.
pub fn webhook_router(config: BotConfig) -> Router {
    Router::new()
        .route("/", post(handle_webhook))
        .with_state(config)
}

#[derive(Deserialize)]
struct InstallationInfo {
    id: i64,
}

#[derive(Deserialize)]
struct WebhookPayload {
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
}

async fn handle_webhook(
    State(config): State<BotConfig>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Replay attack protection: check X-GitHub-Delivery
    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !delivery_id.is_empty() {
        if let Ok(mut tracker) = DELIVERY_TRACKER.lock() {
            if tracker.is_duplicate(delivery_id) {
                return Ok(Json(serde_json::json!({"status": "ok", "duplicate": true})));
            }
        }
    }

    let sig = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify::verify_signature(&config.webhook_secret, &body, sig) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let payload: WebhookPayload =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    if event == "pull_request"
        && matches!(
            payload.action.as_str(),
            "opened" | "reopened" | "synchronize" | "ready_for_review"
        )
    {
        if payload.pull_request.is_some() {
            let pr = payload.pull_request.as_ref().cloned();
            let ctx = WebhookContext::from_payload(config.clone(), &payload);
            tokio::spawn(async move {
                let _ = timeout(Duration::from_secs(300), async move {
                let token = match get_installation_token(&ctx.cfg, ctx.inst_id).await {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("  Auth error: {}", e);
                        return;
                    }
                };
                let wrapped = WebhookPayload {
                    action: String::new(),
                    pull_request: pr,
                    repo: None,
                    installation: None,
                    comment: None,
                    issue: None,
                };
                if let Err(e) = review_pr(&token, &ctx.repo_full_name, &wrapped).await {
                    eprintln!("  Review error: {}", e);
                }
                }).await;
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
            || comment_body.contains("@codasaurus ignore");

        if is_review_cmd || is_ignore_cmd {
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
                    tokio::spawn(spawn_review(ctx, pr_number));
                } else {
                    tokio::spawn(spawn_ignore_comment(ctx, pr_number));
                }
            }
        }
    }
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn spawn_review(ctx: WebhookContext, pr_number: i64) {
    let _ = timeout(Duration::from_secs(300), async move {
    let token = match get_installation_token(&ctx.cfg, ctx.inst_id).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  Auth error: {}", e);
            return;
        }
    };
    if pr_number <= 0 || ctx.repo_full_name == "unknown" {
        eprintln!("  Ignoring review command without a valid repository and PR number");
        return;
    }
    let pr_data = match fetch_pull_request(&token, &ctx.repo_full_name, pr_number).await {
        Ok(data) => data,
        Err(e) => {
            eprintln!("  Failed to fetch PR #{}: {}", pr_number, e);
            return;
        }
    };
    let wrapped = WebhookPayload {
        action: String::new(),
        pull_request: Some(pr_data),
        repo: None,
        installation: None,
        comment: None,
        issue: None,
    };
    if let Err(e) = review_pr(&token, &ctx.repo_full_name, &wrapped).await {
        eprintln!("  Review error: {}", e);
    }
    }).await;
}

async fn spawn_ignore_comment(ctx: WebhookContext, pr_number: i64) {
    let _ = timeout(Duration::from_secs(120), async move {
    let token = match get_installation_token(&ctx.cfg, ctx.inst_id).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  Auth error: {}", e);
            return;
        }
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let url = format!(
        "https://api.github.com/repos/{}/issues/{}/comments",
        ctx.repo_full_name, pr_number
    );
    let body = "👋 Codasaurus ignore is now available as a placeholder. \
        To dismiss specific findings, use `@codasaurus-bot dismiss <fingerprint>` \
        or reply to an inline comment with `@codasaurus-bot ignore`. \
        Full per-finding dismissal will be available in a future release.";
    let _ = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", USER_AGENT)
        .json(&serde_json::json!({"body": body}))
        .send()
        .await;
    }).await;
}
