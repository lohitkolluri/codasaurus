use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tokio::signal;
use tower_http::limit::RequestBodyLimitLayer;

mod auth;
mod review;
mod verify;

#[derive(Clone)]
pub struct BotConfig {
    pub app_id: String,
    pub private_key: String,
    pub webhook_secret: String,
    pub host: String,
    pub port: u16,
}

pub async fn serve(config: BotConfig) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    println!("  Bot listening on {}", addr);
    let app = Router::new()
        .route("/health", get(health))
        .route("/webhook", post(handle_webhook))
        .with_state(config)
        .layer(RequestBodyLimitLayer::new(1024 * 1024));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("  Shutting down (Ctrl+C)...");
        }
        _ = terminate => {
            println!("  Shutting down (SIGTERM)...");
        }
    }
}

async fn health() -> &'static str {
    "ok"
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
        if let Some(pr) = payload.pull_request {
            let cfg = config.clone();
            let inst_id = payload.installation.as_ref().map(|i| i.id);
            let repo_name = payload
                .repo
                .as_ref()
                .and_then(|r| r["full_name"].as_str())
                .unwrap_or("unknown")
                .to_string();
            tokio::spawn(async move {
                let token = auth::get_installation_token(&cfg, inst_id).await;
                match token {
                    Ok(t) => {
                        let wrapped = WebhookPayload {
                            action: String::new(),
                            pull_request: Some(pr),
                            repo: None,
                            installation: None,
                            comment: None,
                            issue: None,
                        };
                        if let Err(e) = review::review_pr(&t, &repo_name, &wrapped).await {
                            eprintln!("  Review error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("  Auth error: {}", e),
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
            || comment_body.contains("@codasaurus full review");

        if is_review_cmd {
            let is_pr = payload
                .issue
                .as_ref()
                .and_then(|i| i.get("pull_request"))
                .is_some();

            if is_pr {
                let cfg = config.clone();
                let inst_id = payload.installation.as_ref().map(|i| i.id);
                let pr_number = payload
                    .issue
                    .as_ref()
                    .and_then(|i| i["number"].as_i64())
                    .unwrap_or(0);
                let repo_full_name = payload
                    .repo
                    .as_ref()
                    .and_then(|r| r["full_name"].as_str())
                    .unwrap_or("unknown")
                    .to_string();

                tokio::spawn(async move {
                    let token = match auth::get_installation_token(&cfg, inst_id).await {
                        Ok(t) => t,
                        Err(e) => {
                            eprintln!("  Auth error: {}", e);
                            return;
                        }
                    };

                    if pr_number <= 0 || repo_full_name == "unknown" {
                        eprintln!(
                            "  Ignoring review command without a valid repository and PR number"
                        );
                        return;
                    }
                    let repo_name = &repo_full_name;
                    let pr_data =
                        match review::fetch_pull_request(&token, repo_name, pr_number).await {
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

                    if let Err(e) = review::review_pr(&token, repo_name, &wrapped).await {
                        eprintln!("  Review error: {}", e);
                    }
                });
            }
        }
    }
    Ok(Json(serde_json::json!({"status": "ok"})))
}
