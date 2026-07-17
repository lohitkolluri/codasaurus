use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
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
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Deserialize)]
struct WebhookPayload {
    #[serde(rename = "action")]
    _action: String,
    #[serde(rename = "pull_request")]
    pull_request: Option<serde_json::Value>,
    #[serde(rename = "repository")]
    _repo: Option<serde_json::Value>,
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
    if event == "pull_request" {
        if let Some(pr) = payload.pull_request {
            let cfg = config.clone();
            tokio::spawn(async move {
                let token = auth::get_installation_token(&cfg).await;
                match token {
                    Ok(t) => {
                        let wrapped = WebhookPayload {
                            _action: String::new(),
                            pull_request: Some(pr),
                            _repo: None,
                        };
                        if let Err(e) = review::review_pr(&t, &wrapped).await {
                            eprintln!("  Review error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("  Auth error: {}", e),
                }
            });
        }
    }
    Ok(Json(serde_json::json!({"status": "ok"})))
}
